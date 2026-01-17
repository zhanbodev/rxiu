//! Daemon TCP server.

use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, RwLock};

use crate::p2p::service::P2PService;
use crate::rs::RsStore;
use crate::storage::ZoneManager;
use crate::daemon::rs_sync::RsSyncManager;

use super::protocol::{encode_message, DaemonRequest, DaemonResponse, DAEMON_PORT};

/// Run the daemon server.
pub async fn run_server(
    p2p: P2PService,
    zone_manager: Arc<RwLock<ZoneManager>>,
    rs_store: Arc<RwLock<RsStore>>,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> crate::Result<()> {
    let addr = format!("127.0.0.1:{}", DAEMON_PORT);
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("Daemon listening on {}", addr);

    let sync_manager = RsSyncManager::start(p2p.clone(), rs_store.clone());
    
    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, addr)) => {
                        tracing::debug!("Client connected from {}", addr);
                        let p2p = p2p.clone();
                        let zm = zone_manager.clone();
                        let rs = rs_store.clone();
                        let sync_manager = sync_manager.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_client(stream, p2p, zm, rs, sync_manager).await {
                                tracing::warn!("Client error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        tracing::error!("Accept error: {}", e);
                    }
                }
            }
            _ = shutdown_rx.recv() => {
                tracing::info!("Daemon shutdown signal received");
                break;
            }
        }
    }
    
    Ok(())
}

async fn handle_client(
    mut stream: TcpStream,
    p2p: P2PService,
    _zone_manager: Arc<RwLock<ZoneManager>>,
    _rs_store: Arc<RwLock<RsStore>>,
    sync_manager: RsSyncManager,
) -> crate::Result<()> {
    loop {
        // Read length prefix
        let mut len_buf = [0u8; 4];
        if stream.read_exact(&mut len_buf).await.is_err() {
            break; // Client disconnected
        }
        
        let len = u32::from_be_bytes(len_buf) as usize;
        if len > 100 * 1024 * 1024 {
            // Sanity check: max 100MB message
            break;
        }
        
        // Read payload
        let mut payload = vec![0u8; len];
        stream.read_exact(&mut payload).await?;
        
        // Parse request (CBOR)
        let request: DaemonRequest = match cbor4ii::serde::from_slice(&payload) {
            Ok(req) => req,
            Err(e) => {
                let resp = DaemonResponse::Error(format!("Invalid request: {}", e));
                let data = encode_message(&resp);
                stream.write_all(&data).await?;
                continue;
            }
        };
        
        // Handle request
        let response = handle_request(request, &p2p, &sync_manager).await;
        
        // Check if shutdown
        let is_shutdown = matches!(response, DaemonResponse::ShuttingDown);
        
        // Send response
        let data = encode_message(&response);
        stream.write_all(&data).await?;
        
        if is_shutdown {
            // Give time for response to be sent, then exit process
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            std::process::exit(0);
        }
    }
    
    Ok(())
}

async fn handle_request(request: DaemonRequest, p2p: &P2PService, sync_manager: &RsSyncManager) -> DaemonResponse {
    match request {
        DaemonRequest::Ping => DaemonResponse::Pong,
        
        DaemonRequest::Shutdown => DaemonResponse::ShuttingDown,
        
        DaemonRequest::GetLocalPeerId => {
            DaemonResponse::LocalPeerId(p2p.local_peer_id().to_string())
        }
        
        DaemonRequest::GetPeers => {
            let peers = p2p.get_peers().await;
            DaemonResponse::Peers(peers)
        }
        
        DaemonRequest::GetPeerCount => {
            let count = p2p.peer_count().await;
            DaemonResponse::PeerCount(count)
        }
        
        DaemonRequest::ListRemoteZones { peer_id } => {
            match peer_id.parse() {
                Ok(pid) => match p2p.list_remote_zones(pid).await {
                    Ok(zones) => DaemonResponse::Zones(zones),
                    Err(e) => DaemonResponse::Error(e.to_string()),
                },
                Err(_) => DaemonResponse::Error("Invalid peer ID".to_string()),
            }
        }
        
        DaemonRequest::ListRemoteFiles { peer_id, zone } => {
            match peer_id.parse() {
                Ok(pid) => match p2p.list_remote_files(pid, &zone).await {
                    Ok(files) => DaemonResponse::Files(files),
                    Err(e) => DaemonResponse::Error(e.to_string()),
                },
                Err(_) => DaemonResponse::Error("Invalid peer ID".to_string()),
            }
        }
        
        DaemonRequest::FetchFile { peer_id, zone, name } => {
            match peer_id.parse() {
                Ok(pid) => match p2p.fetch_file(pid, &zone, &name).await {
                    Ok((content, hash)) => DaemonResponse::FileData { content, hash },
                    Err(e) => DaemonResponse::Error(e.to_string()),
                },
                Err(_) => DaemonResponse::Error("Invalid peer ID".to_string()),
            }
        }
        
        DaemonRequest::GetFileMeta { peer_id, zone, name } => {
            match peer_id.parse() {
                Ok(pid) => match p2p.get_file_meta(pid, &zone, &name).await {
                    Ok(meta) => DaemonResponse::FileMeta(meta),
                    Err(e) => DaemonResponse::Error(e.to_string()),
                },
                Err(_) => DaemonResponse::Error("Invalid peer ID".to_string()),
            }
        }
        
        DaemonRequest::GetFileChunk { peer_id, zone, name, offset, size } => {
            match peer_id.parse() {
                Ok(pid) => match p2p.get_file_chunk(pid, &zone, &name, offset, size).await {
                    Ok(chunk) => DaemonResponse::FileChunk(chunk),
                    Err(e) => DaemonResponse::Error(e.to_string()),
                },
                Err(_) => DaemonResponse::Error("Invalid peer ID".to_string()),
            }
        }
        
        DaemonRequest::RsList { peer_id } => {
            match peer_id.parse() {
                Ok(pid) => match p2p.rs_list(pid).await {
                    Ok(files) => DaemonResponse::RsFiles(files),
                    Err(e) => DaemonResponse::Error(e.to_string()),
                },
                Err(_) => DaemonResponse::Error("Invalid peer ID".to_string()),
            }
        }
        
        DaemonRequest::RsAnnounce { peer_id, file } => {
            match peer_id.parse() {
                Ok(pid) => match p2p.rs_announce(pid, file).await {
                    Ok(()) => DaemonResponse::Ok,
                    Err(e) => DaemonResponse::Error(e.to_string()),
                },
                Err(_) => DaemonResponse::Error("Invalid peer ID".to_string()),
            }
        }
        
        DaemonRequest::RsGetMeta { peer_id, name } => {
            match peer_id.parse() {
                Ok(pid) => match p2p.rs_get_meta(pid, &name).await {
                    Ok(meta) => DaemonResponse::RsMeta(meta),
                    Err(e) => DaemonResponse::Error(e.to_string()),
                },
                Err(_) => DaemonResponse::Error("Invalid peer ID".to_string()),
            }
        }
        
        DaemonRequest::RsGetBlock { peer_id, hash } => {
            match peer_id.parse() {
                Ok(pid) => match p2p.rs_get_block(pid, &hash).await {
                    Ok(block) => DaemonResponse::RsBlock(block),
                    Err(e) => DaemonResponse::Error(e.to_string()),
                },
                Err(_) => DaemonResponse::Error("Invalid peer ID".to_string()),
            }
        }
        
        DaemonRequest::RsGetBlocks { peer_id, hashes } => {
            match peer_id.parse() {
                Ok(pid) => match p2p.rs_get_blocks(pid, hashes).await {
                    Ok(blocks) => DaemonResponse::RsBlocks(blocks),
                    Err(e) => DaemonResponse::Error(e.to_string()),
                },
                Err(_) => DaemonResponse::Error("Invalid peer ID".to_string()),
            }
        }
        
        DaemonRequest::RsHave { peer_id, name } => {
            match peer_id.parse() {
                Ok(pid) => match p2p.rs_have(pid, &name).await {
                    Ok(have) => DaemonResponse::RsHave(have),
                    Err(e) => DaemonResponse::Error(e.to_string()),
                },
                Err(_) => DaemonResponse::Error("Invalid peer ID".to_string()),
            }
        }
        
        DaemonRequest::RsDelete { peer_id, name } => {
            match peer_id.parse() {
                Ok(pid) => match p2p.rs_delete(pid, &name).await {
                    Ok(()) => DaemonResponse::Ok,
                    Err(e) => DaemonResponse::Error(e.to_string()),
                },
                Err(_) => DaemonResponse::Error("Invalid peer ID".to_string()),
            }
        }
        DaemonRequest::RsSync => {
            sync_manager.trigger().await;
            DaemonResponse::Ok
        }
        DaemonRequest::RsSyncStatus => {
            let status = sync_manager.status().await;
            DaemonResponse::RsSyncStatus(status)
        }
        DaemonRequest::RefreshPeers => {
            match p2p.refresh_peers().await {
                Ok(count) => DaemonResponse::PeerCount(count),
                Err(e) => DaemonResponse::Error(e.to_string()),
            }
        }
    }
}
