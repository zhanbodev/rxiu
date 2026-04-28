//! Renew (auto-update) manager for the daemon.

use std::sync::Arc;
use std::time::Duration;

use libp2p::PeerId;
use tokio::sync::Mutex;

use crate::config::AppConfig;
use crate::error::Result;
use crate::p2p::service::P2PService;
use crate::renew::VersionInfo;

/// State for the renew manager.
struct RenewState {
    config: AppConfig,
    local_version: Option<VersionInfo>,
    last_check: std::time::Instant,
    update_in_progress: bool,
}

/// Renew manager that runs in the daemon.
#[derive(Clone)]
pub struct RenewManager {
    state: Arc<Mutex<RenewState>>,
}

impl RenewManager {
    /// Create a new renew manager.
    pub fn start(p2p: P2PService) -> Self {
        let config = AppConfig::load().unwrap_or_default();
        let local_version = VersionInfo::current().ok();

        let state = Arc::new(Mutex::new(RenewState {
            config,
            local_version,
            last_check: std::time::Instant::now(),
            update_in_progress: false,
        }));

        let state_clone = state.clone();
        let p2p_clone = p2p.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));

            loop {
                interval.tick().await;

                // Reload config
                if let Ok(config) = AppConfig::load() {
                    let mut guard = state_clone.lock().await;
                    guard.config = config;
                }

                // Check if renew is enabled
                let (enabled, check_interval) = {
                    let guard = state_clone.lock().await;
                    (
                        guard.config.renew_enabled,
                        guard.config.renew_check_interval,
                    )
                };

                if !enabled {
                    continue;
                }

                // Check if it's time to check for updates
                {
                    let guard = state_clone.lock().await;
                    let elapsed = guard.last_check.elapsed().as_secs();
                    if elapsed < check_interval {
                        continue;
                    }
                }

                // Perform update check
                if let Err(e) = check_for_updates(&state_clone, &p2p_clone).await {
                    tracing::warn!("[Renew] Update check failed: {}", e);
                }
            }
        });

        Self { state }
    }

    /// Get current version info.
    pub async fn current_version(&self) -> Option<VersionInfo> {
        let guard = self.state.lock().await;
        guard.local_version.clone()
    }
}

/// Check for updates from peers.
async fn check_for_updates(state: &Arc<Mutex<RenewState>>, p2p: &P2PService) -> Result<()> {
    // Update last check time
    {
        let mut guard = state.lock().await;
        guard.last_check = std::time::Instant::now();
    }

    let local_version = {
        let guard = state.lock().await;
        guard.local_version.clone()
    };

    let Some(local_version) = local_version else {
        tracing::warn!("[Renew] Cannot check for updates: local version unknown");
        return Ok(());
    };

    // Get all peers
    let peers = p2p.get_peers().await;
    if peers.is_empty() {
        return Ok(());
    }

    tracing::info!(
        "[Renew] Checking {} peers for updates (current: {})",
        peers.len(),
        local_version.version
    );

    // Query version from each peer
    for peer_info in peers {
        let peer_id = peer_info.peer_id;

        // Request version info
        match p2p.renew_get_version(peer_id).await {
            Ok(remote_version) => {
                // Check platform compatibility first
                if !remote_version.is_compatible_with(&local_version) {
                    tracing::debug!(
                        "[Renew] Skipping {} - incompatible platform: {}/{} (local: {}/{})",
                        peer_id,
                        remote_version.target_os,
                        remote_version.target_arch,
                        local_version.target_os,
                        local_version.target_arch
                    );
                    continue;
                }

                if remote_version.is_newer_than(&local_version) {
                    tracing::info!(
                        "[Renew] Found newer version on {}: {} (local: {})",
                        peer_id,
                        remote_version.version,
                        local_version.version
                    );

                    // Start update download
                    if let Err(e) =
                        download_and_apply_update(state, p2p, peer_id, &remote_version).await
                    {
                        tracing::error!("[Renew] Update failed: {}", e);
                    }
                    return Ok(());
                }
            }
            Err(e) => {
                tracing::debug!("[Renew] Failed to get version from {}: {}", peer_id, e);
            }
        }
    }

    tracing::debug!("[Renew] No updates available");
    Ok(())
}

/// Download and apply an update from a peer.
async fn download_and_apply_update(
    state: &Arc<Mutex<RenewState>>,
    p2p: &P2PService,
    peer_id: PeerId,
    version: &VersionInfo,
) -> Result<()> {
    use sha2::{Digest, Sha256};
    use std::io::Write;

    // Check if update already in progress
    {
        let mut guard = state.lock().await;
        if guard.update_in_progress {
            return Ok(());
        }
        guard.update_in_progress = true;
    }

    // Cleanup on exit
    let state_clone = state.clone();
    let _cleanup = scopeguard::guard((), |_| {
        tokio::spawn(async move {
            let mut guard = state_clone.lock().await;
            guard.update_in_progress = false;
        });
    });

    tracing::info!(
        "[Renew] Downloading update {} ({} bytes)",
        version.version,
        version.size
    );

    // Create staging directory
    let staging_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".rxiu")
        .join("updates");
    std::fs::create_dir_all(&staging_dir)?;

    let staging_path = staging_dir.join(format!("rxiu-daemon.{}", &version.hash[..8]));
    let mut file = std::fs::File::create(&staging_path)?;
    let mut hasher = Sha256::new();

    // Download in chunks
    let chunk_size = 1024 * 1024u32; // 1MB
    let mut offset = 0u64;

    loop {
        match p2p
            .renew_get_binary_chunk(peer_id, offset, chunk_size)
            .await
        {
            Ok((data, is_last)) => {
                if data.is_empty() && is_last {
                    break;
                }
                file.write_all(&data)?;
                hasher.update(&data);
                offset += data.len() as u64;

                if is_last {
                    break;
                }
            }
            Err(e) => {
                // Clean up partial file
                let _ = std::fs::remove_file(&staging_path);
                return Err(e);
            }
        }
    }

    file.sync_all()?;
    drop(file);

    // Verify hash
    let actual_hash = format!("{:x}", hasher.finalize());
    if actual_hash != version.hash {
        let _ = std::fs::remove_file(&staging_path);
        return Err(crate::error::AppError::Io(std::io::Error::other(format!(
            "Hash mismatch: expected {}, got {}",
            version.hash, actual_hash
        ))));
    }

    tracing::info!("[Renew] Download complete, applying update...");

    // Apply update
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::process::CommandExt;

        let current_exe = std::env::current_exe()?;

        // Create backup
        let backup_path = current_exe.with_extension("bak");
        std::fs::copy(&current_exe, &backup_path)?;

        // Set executable permissions
        let mut perms = std::fs::metadata(&staging_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&staging_path, perms)?;

        // Atomic rename
        std::fs::rename(&staging_path, &current_exe)?;

        tracing::info!("[Renew] Update applied! Restarting daemon...");

        // Self-restart using exec
        // This replaces the current process with a new instance
        let args: Vec<String> = std::env::args().collect();
        let err = std::process::Command::new(&current_exe)
            .args(&args[1..])
            .exec();

        // exec() only returns on error
        tracing::error!("[Renew] Failed to restart: {}", err);
        return Err(crate::error::AppError::Io(std::io::Error::other(format!(
            "Failed to restart daemon: {}",
            err
        ))));
    }

    #[cfg(not(unix))]
    {
        // Windows: spawn new process then exit
        // Windows doesn't have exec(), so we spawn a new daemon and exit

        let current_exe = std::env::current_exe()?;

        // Create backup
        let backup_path = current_exe.with_extension("bak");
        std::fs::copy(&current_exe, &backup_path)?;

        // On Windows, we can't replace a running executable directly
        // So we rename current to .old, move new to current, then restart
        let old_path = current_exe.with_extension("old");
        let _ = std::fs::remove_file(&old_path); // Remove any previous .old
        std::fs::rename(&current_exe, &old_path)?;
        std::fs::rename(&staging_path, &current_exe)?;

        tracing::info!("[Renew] Update applied! Restarting daemon...");

        // Spawn new daemon process
        let args: Vec<String> = std::env::args().collect();
        match std::process::Command::new(&current_exe)
            .args(&args[1..])
            .spawn()
        {
            Ok(_child) => {
                tracing::info!("[Renew] New daemon started, exiting old process");
                // Give the new process time to start
                std::thread::sleep(std::time::Duration::from_millis(500));
                std::process::exit(0);
            }
            Err(e) => {
                // Rollback
                tracing::error!("[Renew] Failed to start new daemon: {}", e);
                let _ = std::fs::rename(&old_path, &current_exe);
                return Err(crate::error::AppError::Io(std::io::Error::other(format!(
                    "Failed to restart daemon: {}",
                    e
                ))));
            }
        }
    }
}
