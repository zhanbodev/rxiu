//! File metadata representation.
//!
//! Separates file metadata from content, enabling content-addressable storage
//! patterns where the same content can be deduplicated across zones.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Metadata for a file stored in a zone.
///
/// The `content_hash` field stores a SHA-256 hash of the file content,
/// which can be used as a content identifier for future CAS backends.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    /// Original filename
    pub name: String,
    /// File size in bytes
    pub size: u64,
    /// SHA-256 hash of content (hex-encoded)
    pub content_hash: String,
    /// When the original file was created (if available)
    pub created_at: Option<DateTime<Utc>>,
    /// When the file was imported into the zone
    pub imported_at: DateTime<Utc>,
}

impl FileMetadata {
    /// Format size for human-readable display.
    pub fn formatted_size(&self) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;

        if self.size >= GB {
            format!("{:.2} GB", self.size as f64 / GB as f64)
        } else if self.size >= MB {
            format!("{:.2} MB", self.size as f64 / MB as f64)
        } else if self.size >= KB {
            format!("{:.2} KB", self.size as f64 / KB as f64)
        } else {
            format!("{} B", self.size)
        }
    }
}
