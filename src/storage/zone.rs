//! Zone abstraction.
//!
//! A Zone represents a logical namespace for files, built on top of a
//! StorageBackend. This separation allows zones to be backed by different
//! storage implementations.

use std::sync::Arc;

use super::{FileMetadata, StorageBackend};
use crate::error::Result;

/// A logical file zone backed by a storage implementation.
pub struct Zone {
    name: String,
    backend: Arc<dyn StorageBackend>,
}

impl Zone {
    /// Create a zone with the given backend.
    pub fn new(name: String, backend: Arc<dyn StorageBackend>) -> Self {
        Self { name, backend }
    }

    /// Get the zone name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Store a file in the zone.
    pub fn store(&self, name: &str, content: &[u8]) -> Result<FileMetadata> {
        self.backend.store(name, content)
    }

    /// Retrieve a file from the zone.
    pub fn retrieve(&self, name: &str) -> Result<Vec<u8>> {
        self.backend.retrieve(name)
    }

    /// Retrieve a chunk from a file in the zone.
    pub fn read_chunk(&self, name: &str, offset: u64, size: u64) -> Result<Vec<u8>> {
        self.backend.read_chunk(name, offset, size)
    }

    /// Delete a file from the zone.
    #[allow(dead_code)]
    pub fn delete(&self, name: &str) -> Result<()> {
        self.backend.delete(name)
    }

    /// List all files in the zone.
    pub fn list(&self) -> Result<Vec<FileMetadata>> {
        self.backend.list()
    }

    /// Check if a file exists.
    pub fn exists(&self, name: &str) -> bool {
        self.backend.exists(name)
    }
}
