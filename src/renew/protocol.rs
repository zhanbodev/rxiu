//! P2P protocol messages for auto-update.

use serde::{Deserialize, Serialize};

use super::version::VersionInfo;

/// Request messages for the renew protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RenewRequest {
    /// Request version information from a peer.
    GetVersion,
    /// Request a chunk of the binary.
    GetBinaryChunk {
        /// Offset in bytes from start of binary.
        offset: u64,
        /// Maximum number of bytes to return.
        length: u32,
    },
}

/// Response messages for the renew protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RenewResponse {
    /// Version information.
    Version(VersionInfo),
    /// A chunk of the binary.
    BinaryChunk {
        /// Offset in bytes from start of binary.
        offset: u64,
        /// Binary data.
        data: Vec<u8>,
        /// Whether this is the last chunk.
        is_last: bool,
    },
    /// Error response.
    Error(String),
}

/// Default chunk size for binary transfer (1MB).
pub const CHUNK_SIZE: u32 = 1024 * 1024;
