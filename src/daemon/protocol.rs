//! IPC protocol definitions for daemon communication.
//!
//! Uses JSON over TCP for cross-platform compatibility.

use serde::{Deserialize, Serialize};

use crate::p2p::messages::{FileChunk, FileMeta, RsBlock, RsHave};
use crate::p2p::node::PeerInfo;
use crate::rs::RsFileEntry;
use crate::storage::FileMetadata;

/// Default daemon port.
pub const DAEMON_PORT: u16 = 19820;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RsSyncStatus {
    pub in_progress: bool,
    pub last_updated_files: usize,
    pub last_error: Option<String>,
    pub global_sync: bool,
    pub download_concurrency: usize,
    pub sync_concurrency: usize,
    pub block_size_mb: u64,
}

/// Daemon request types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DaemonRequest {
    /// Health check.
    Ping,
    /// Shutdown the daemon.
    Shutdown,
    /// Get local peer ID.
    GetLocalPeerId,
    /// Get discovered peers.
    GetPeers,
    /// Get peer count.
    GetPeerCount,
    /// List zones on a remote peer.
    ListRemoteZones { peer_id: String },
    /// List files in a remote zone.
    ListRemoteFiles { peer_id: String, zone: String },
    /// Fetch a file from a remote peer.
    FetchFile {
        peer_id: String,
        zone: String,
        name: String,
    },
    /// Get file metadata.
    GetFileMeta {
        peer_id: String,
        zone: String,
        name: String,
    },
    /// Get file chunk.
    GetFileChunk {
        peer_id: String,
        zone: String,
        name: String,
        offset: u64,
        size: u64,
    },
    /// RS: list files from a peer.
    RsList { peer_id: String },
    /// RS: announce a file.
    RsAnnounce { peer_id: String, file: RsFileEntry },
    /// RS: get file metadata.
    RsGetMeta { peer_id: String, name: String },
    /// RS: get a block.
    RsGetBlock { peer_id: String, hash: String },
    /// RS: get multiple blocks.
    RsGetBlocks {
        peer_id: String,
        hashes: Vec<String>,
    },
    /// RS: ask which blocks a peer has for a file.
    RsHave { peer_id: String, name: String },
    /// RS: delete a file.
    RsDelete { peer_id: String, name: String },
    /// RS: trigger block sync.
    RsSync,
    /// RS: sync status.
    RsSyncStatus,
    /// Refresh LAN peers (trigger network recovery).
    RefreshPeers,
}

/// Daemon response types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DaemonResponse {
    /// Pong response.
    Pong,
    /// Shutdown acknowledged.
    ShuttingDown,
    /// Local peer ID.
    LocalPeerId(String),
    /// List of peers.
    Peers(Vec<PeerInfo>),
    /// Peer count.
    PeerCount(usize),
    /// List of zones.
    Zones(Vec<String>),
    /// List of files.
    Files(Vec<FileMetadata>),
    /// File data.
    FileData { content: Vec<u8>, hash: String },
    /// File metadata.
    FileMeta(FileMeta),
    /// File chunk.
    FileChunk(FileChunk),
    /// RS files list.
    RsFiles(Vec<RsFileEntry>),
    /// RS file metadata.
    RsMeta(RsFileEntry),
    /// RS block.
    RsBlock(RsBlock),
    /// RS blocks.
    RsBlocks(Vec<RsBlock>),
    /// RS block availability.
    RsHave(RsHave),
    /// RS sync status.
    RsSyncStatus(RsSyncStatus),
    /// Success with no data.
    Ok,
    /// Error response.
    Error(String),
}

/// Framed message format: 4-byte length prefix + CBOR payload.
pub fn encode_message<T: Serialize>(msg: &T) -> Vec<u8> {
    let cbor = cbor4ii::serde::to_vec(Vec::new(), msg).unwrap_or_default();
    let len = (cbor.len() as u32).to_be_bytes();
    let mut buf = Vec::with_capacity(4 + cbor.len());
    buf.extend_from_slice(&len);
    buf.extend_from_slice(&cbor);
    buf
}

/// Parse length prefix from buffer.
pub fn parse_length(buf: &[u8]) -> Option<usize> {
    if buf.len() < 4 {
        return None;
    }
    let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    Some(len)
}
