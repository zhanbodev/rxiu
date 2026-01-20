//! RS sync manager running inside the daemon.

use std::sync::Arc;
use std::time::Duration;

use futures::stream::{FuturesUnordered, StreamExt};
use tokio::sync::{Mutex, RwLock, Semaphore, mpsc};

use crate::config::AppConfig;
use crate::daemon::protocol::RsSyncStatus;
use crate::error::{AppError, Result};
use crate::p2p::service::P2PService;
use crate::rs::RsStore;
use crate::rs::sync::{entry_members, is_block_assigned_to, needs_sync};

#[derive(Debug)]
struct RsSyncState {
    in_progress: bool,
    last_updated_files: usize,
    last_error: Option<String>,
    config: AppConfig,
}

#[derive(Clone)]
pub struct RsSyncManager {
    state: Arc<Mutex<RsSyncState>>,
    trigger_tx: mpsc::Sender<()>,
}

impl RsSyncManager {
    pub fn start(p2p: P2PService, rs_store: Arc<RwLock<RsStore>>) -> Self {
        let config = AppConfig::load().unwrap_or_default();
        let state = Arc::new(Mutex::new(RsSyncState {
            in_progress: false,
            last_updated_files: 0,
            last_error: None,
            config,
        }));
        let (trigger_tx, mut trigger_rx) = mpsc::channel::<()>(4);
        let state_clone = state.clone();
        let p2p_clone = p2p.clone();
        let rs_store_clone = rs_store.clone();

        tokio::spawn(async move {
            let mut config_tick = tokio::time::interval(Duration::from_secs(2));
            let mut auto_tick = tokio::time::interval(Duration::from_secs(5));
            loop {
                tokio::select! {
                    _ = config_tick.tick() => {
                        if let Ok(config) = AppConfig::load() {
                            let mut state = state_clone.lock().await;
                            state.config = config;
                        }
                    }
                    _ = auto_tick.tick() => {
                        let (enabled, concurrency) = {
                            let state = state_clone.lock().await;
                            (state.config.rs_global_sync, state.config.rs_sync_concurrency)
                        };
                        if enabled {
                            let _ = run_sync_if_needed(&state_clone, &p2p_clone, &rs_store_clone, concurrency, false).await;
                        }
                    }
                    Some(_) = trigger_rx.recv() => {
                        let concurrency = {
                            let state = state_clone.lock().await;
                            state.config.rs_sync_concurrency
                        };
                        let _ = run_sync_if_needed(&state_clone, &p2p_clone, &rs_store_clone, concurrency, true).await;
                    }
                }
            }
        });

        Self { state, trigger_tx }
    }

    pub async fn trigger(&self) {
        let _ = self.trigger_tx.send(()).await;
    }

    pub async fn status(&self) -> RsSyncStatus {
        let state = self.state.lock().await;
        RsSyncStatus {
            in_progress: state.in_progress,
            last_updated_files: state.last_updated_files,
            last_error: state.last_error.clone(),
            global_sync: state.config.rs_global_sync,
            download_concurrency: state.config.rs_sync_concurrency,
            sync_concurrency: state.config.rs_sync_concurrency,
            block_size_mb: state.config.rs_block_size_mb,
        }
    }
}

async fn run_sync_if_needed(
    state: &Arc<Mutex<RsSyncState>>,
    p2p: &P2PService,
    rs_store: &Arc<RwLock<RsStore>>,
    concurrency: usize,
    force: bool,
) -> Result<()> {
    {
        let mut guard = state.lock().await;
        if guard.in_progress {
            return Ok(());
        }
        guard.in_progress = true;
        guard.last_error = None;
    }

    let result = if force {
        let rf = { state.lock().await.config.rs_replication_factor };
        sync_rs_missing_blocks(p2p, rs_store, concurrency, rf).await
    } else {
        let peers = p2p.get_peers().await;
        let local_id = p2p.local_peer_id().to_string();
        let mut members: Vec<String> = peers.iter().map(|p| p.peer_id.to_string()).collect();
        members.push(local_id.clone());
        members.sort();
        if members.is_empty() {
            Ok(0)
        } else {
            let rf = { state.lock().await.config.rs_replication_factor };
            let store = rs_store.read().await;
            let needs = needs_sync(&store, &local_id, &members, rf)?;
            drop(store);
            if needs {
                sync_rs_missing_blocks(p2p, rs_store, concurrency, rf).await
            } else {
                Ok(0)
            }
        }
    };

    let mut guard = state.lock().await;
    guard.in_progress = false;
    match result {
        Ok(updated) => {
            guard.last_updated_files = updated;
        }
        Err(e) => {
            guard.last_error = Some(e.to_string());
        }
    }
    Ok(())
}

async fn sync_rs_missing_blocks(
    p2p: &P2PService,
    rs_store: &Arc<RwLock<RsStore>>,
    concurrency: usize,
    replication_factor: usize,
) -> Result<usize> {
    let peers = p2p.get_peers().await;
    if peers.is_empty() {
        return Ok(0);
    }
    let peer_ids: Vec<_> = peers.iter().map(|p| p.peer_id).collect();
    let local_id = p2p.local_peer_id().to_string();
    let mut fallback: Vec<String> = peer_ids.iter().map(|p| p.to_string()).collect();
    fallback.push(local_id.clone());
    fallback.sort();

    let files = {
        let store = rs_store.read().await;
        store.list_files()?
    };
    if files.is_empty() {
        return Ok(0);
    }

    let semaphore = Arc::new(Semaphore::new(concurrency.max(1)));
    let mut updated_files = 0usize;

    for mut entry in files {
        let members = entry_members(&entry, &fallback);
        let Some(local_index) = members.iter().position(|id| id == &local_id) else {
            entry.complete = false;
            entry.syncing = false;
            rs_store.write().await.upsert_file(entry)?;
            continue;
        };
        let owned_blocks = entry
            .blocks
            .iter()
            .filter(|b| is_block_assigned_to(b, &members, local_index, replication_factor))
            .count();
        let missing: Vec<_> = {
            let store = rs_store.read().await;
            entry
                .blocks
                .iter()
                .filter(|b| is_block_assigned_to(b, &members, local_index, replication_factor))
                .filter(|b| !store.has_block(&b.hash))
                .cloned()
                .collect()
        };
        if missing.is_empty() {
            if owned_blocks > 0 && !entry.complete {
                entry.complete = true;
                entry.syncing = false;
                rs_store.write().await.upsert_file(entry)?;
                updated_files += 1;
            }
            continue;
        }

        rs_store.write().await.set_syncing(&entry.name, true)?;
        let mut tasks = FuturesUnordered::new();
        for (idx, block) in missing.into_iter().enumerate() {
            let sem = semaphore.clone();
            let p2p = p2p.clone();
            let peers = peer_ids.clone();
            let rs_store = rs_store.clone();
            tasks.push(async move {
                let _permit = sem
                    .acquire()
                    .await
                    .map_err(|_| AppError::Io(std::io::Error::other("RS sync semaphore closed")))?;
                if peers.is_empty() {
                    return Err(AppError::Io(std::io::Error::other(
                        "No peers available for RS sync",
                    )));
                }
                let mut fetched = None;
                for cycle in 0..3 {
                    for attempt in 0..peers.len() {
                        let peer = peers[(idx + attempt) % peers.len()];
                        if let Ok(block_data) = p2p.rs_get_block(peer, &block.hash).await {
                            fetched = Some(block_data);
                            break;
                        }
                    }
                    if fetched.is_some() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(200 * (cycle + 1))).await;
                }
                let block_data = fetched.ok_or_else(|| {
                    AppError::Io(std::io::Error::other("Failed to fetch RS block"))
                })?;
                rs_store
                    .read()
                    .await
                    .verify_and_write_block(&block.hash, &block_data.data)?;
                Ok::<(), AppError>(())
            });
        }
        while let Some(result) = tasks.next().await {
            result?;
        }
        let all_have = {
            let store = rs_store.read().await;
            entry
                .blocks
                .iter()
                .filter(|b| is_block_assigned_to(b, &members, local_index, replication_factor))
                .all(|b| store.has_block(&b.hash))
        };
        entry.complete = owned_blocks > 0 && all_have;
        entry.syncing = false;
        rs_store.write().await.upsert_file(entry)?;
        updated_files += 1;
    }

    Ok(updated_files)
}
