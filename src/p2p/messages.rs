//! File transfer protocol definitions.
//!
//! Uses libp2p's request-response pattern for file operations.

use serde::{Deserialize, Serialize};

use crate::renew::VersionInfo;
use crate::rs::RsFileEntry;
use crate::storage::FileMetadata;

pub const FILE_CHUNK_SIZE: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMeta {
    pub zone: String,
    pub name: String,
    pub size: u64,
    pub hash: String,
    pub chunk_size: u64,
    pub chunks: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChunk {
    pub zone: String,
    pub name: String,
    pub offset: u64,
    pub data: Vec<u8>,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RsBlock {
    pub hash: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RsHave {
    pub name: String,
    pub hashes: Vec<String>,
}

// Re-export PeerEntry from protocol module
pub use super::protocol::PeerEntry;

/// Request types for the file protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileRequest {
    /// Heartbeat ping.
    Ping,
    /// List all zones on the remote node.
    ListZones,
    /// List files in a specific zone.
    ListFiles { zone: String },
    /// Request a file's content.
    GetFile { zone: String, name: String },
    /// Request file metadata for chunked download.
    GetFileMeta { zone: String, name: String },
    /// Request a file chunk.
    GetFileChunk {
        zone: String,
        name: String,
        offset: u64,
        size: u64,
    },
    /// RS: list shared files.
    RsList,
    /// RS: announce file metadata.
    RsAnnounce { file: RsFileEntry },
    /// RS: fetch file metadata.
    RsGetMeta { name: String },
    /// RS: fetch a block by hash.
    RsGetBlock { hash: String },
    /// RS: fetch multiple blocks by hash.
    RsGetBlocks { hashes: Vec<String> },
    /// RS: delete a file by name.
    RsDelete { name: String },
    /// RS: ask which blocks a peer has for a file.
    RsHave { name: String },
    /// Get known peers from this node.
    GetPeers,
    /// Renew: get version information.
    RenewGetVersion,
    /// Renew: get a chunk of the binary.
    RenewGetBinaryChunk { offset: u64, length: u32 },
}

/// Response types for the file protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileResponse {
    /// Heartbeat pong.
    Pong,
    /// List of zone names.
    Zones(Vec<String>),
    /// List of files in a zone.
    Files {
        zone: String,
        files: Vec<FileMetadata>,
    },
    /// File content.
    FileData {
        name: String,
        content: Vec<u8>,
        hash: String,
    },
    /// File metadata for chunked download.
    FileMeta(FileMeta),
    /// File chunk for chunked download.
    FileChunk(FileChunk),
    /// RS: list shared files.
    RsFiles(Vec<RsFileEntry>),
    /// RS: file metadata.
    RsMeta(RsFileEntry),
    /// RS: block data.
    RsBlock(RsBlock),
    /// RS: multiple block data.
    RsBlocks(Vec<RsBlock>),
    /// RS: block availability.
    RsHave(RsHave),
    /// RS: ack.
    RsOk,
    /// List of known peers.
    Peers(Vec<PeerEntry>),
    /// Error response.
    Error(String),
    /// Renew: version information.
    RenewVersion(VersionInfo),
    /// Renew: binary chunk.
    RenewBinaryChunk {
        offset: u64,
        data: Vec<u8>,
        is_last: bool,
    },
}

/// Request for zone info.
pub type ZoneRequest = FileRequest;
/// Response for zone info.
pub type ZoneResponse = FileResponse;
