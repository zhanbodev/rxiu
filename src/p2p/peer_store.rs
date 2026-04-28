//! Persistent peer storage.
//!
//! Saves known peers to disk for reconnection after restart.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};

const PEERS_DIR: &str = "peers";
const KNOWN_PEERS_FILE: &str = "known_peers.json";
/// Maximum age for a peer entry (7 days)
const MAX_PEER_AGE_SECS: u64 = 7 * 24 * 60 * 60;

/// A persisted peer entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedPeer {
    pub peer_id: String,
    pub addrs: Vec<String>,
    /// Last seen timestamp (Unix seconds)
    pub last_seen: u64,
}

/// Persistent peer storage.
#[derive(Debug, Default, Serialize, Deserialize)]
struct PeerStore {
    peers: HashMap<String, PersistedPeer>,
}

/// Get the path to the known peers file.
fn peers_file_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or(AppError::NoHomeDirectory)?;
    let peers_dir = home.join(".rxiu").join(PEERS_DIR);
    fs::create_dir_all(&peers_dir)?;
    Ok(peers_dir.join(KNOWN_PEERS_FILE))
}

/// Load known peers from disk.
pub fn load_peers() -> Result<Vec<PersistedPeer>> {
    let path = peers_file_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&path)?;
    let store: PeerStore = serde_json::from_str(&content).unwrap_or_default();

    // Filter out stale peers
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let peers: Vec<PersistedPeer> = store
        .peers
        .into_values()
        .filter(|p| now.saturating_sub(p.last_seen) < MAX_PEER_AGE_SECS)
        .collect();

    Ok(peers)
}

/// Save a peer to disk.
pub fn save_peer(peer_id: &str, addrs: &[String]) -> Result<()> {
    let path = peers_file_path()?;

    // Load existing
    let mut store: PeerStore = if path.exists() {
        let content = fs::read_to_string(&path)?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        PeerStore::default()
    };

    // Update or insert
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    store.peers.insert(
        peer_id.to_string(),
        PersistedPeer {
            peer_id: peer_id.to_string(),
            addrs: addrs.to_vec(),
            last_seen: now,
        },
    );

    // Prune old entries
    store
        .peers
        .retain(|_, p| now.saturating_sub(p.last_seen) < MAX_PEER_AGE_SECS);

    // Save
    let content = serde_json::to_string_pretty(&store)?;
    fs::write(&path, content)?;

    Ok(())
}

/// Remove a peer from disk.
pub fn remove_peer(peer_id: &str) -> Result<()> {
    let path = peers_file_path()?;
    if !path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&path)?;
    let mut store: PeerStore = serde_json::from_str(&content).unwrap_or_default();
    store.peers.remove(peer_id);

    let content = serde_json::to_string_pretty(&store)?;
    fs::write(&path, content)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_store_roundtrip() {
        // This test requires a real filesystem, skip in CI
        if std::env::var("CI").is_ok() {
            return;
        }

        let peer_id = "test_peer_12345";
        let addrs = vec!["/ip4/192.168.1.1/tcp/1234".to_string()];

        save_peer(peer_id, &addrs).unwrap();
        let loaded = load_peers().unwrap();

        assert!(loaded.iter().any(|p| p.peer_id == peer_id));

        remove_peer(peer_id).unwrap();
    }
}
