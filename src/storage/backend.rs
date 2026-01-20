//! Storage backend trait.
//!
//! This trait defines the interface that all storage implementations must provide.
//! By abstracting storage operations behind a trait, we can swap backends without
//! changing the zone or command logic.
//!
//! Future backends might include:
//! - IPFS (content-addressable, distributed)
//! - P2P sync (zone replication across nodes)
//! - Encrypted storage (wrapping another backend)
//! - Cloud storage (S3, GCS)

use super::FileMetadata;
use crate::error::Result;

/// Contract for storage backend implementations.
///
/// All operations are synchronous for simplicity. Async variants could be
/// added via a separate trait or feature flag for high-latency backends.
pub trait StorageBackend: Send + Sync {
    /// Store content under the given name.
    ///
    /// Returns metadata with computed hash and timestamps.
    fn store(&self, name: &str, content: &[u8]) -> Result<FileMetadata>;

    /// Retrieve content by name.
    fn retrieve(&self, name: &str) -> Result<Vec<u8>>;

    /// Retrieve a chunk of a file by name.
    fn read_chunk(&self, name: &str, offset: u64, size: u64) -> Result<Vec<u8>> {
        let content = self.retrieve(name)?;
        let start = offset as usize;
        if start >= content.len() {
            return Err(crate::error::AppError::Io(std::io::Error::other(
                "Chunk offset out of range",
            )));
        }
        let end = (offset + size).min(content.len() as u64) as usize;
        Ok(content[start..end].to_vec())
    }

    /// Delete a file by name.
    fn delete(&self, name: &str) -> Result<()>;

    /// List all files in the backend.
    fn list(&self) -> Result<Vec<FileMetadata>>;

    /// Check if a file exists.
    fn exists(&self, name: &str) -> bool;
}
