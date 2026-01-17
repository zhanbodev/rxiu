//! Local filesystem storage backend.
//!
//! Stores files on the local filesystem under ~/.rxiu/zones/<zone_name>/.
//! Metadata is persisted separately to enable future migration to other backends.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use chrono::Utc;
use sha2::{Sha256, Digest};

use super::{FileMetadata, StorageBackend};
use crate::error::{AppError, Result};

/// Local filesystem implementation of StorageBackend.
pub struct LocalBackend {
    /// Root directory for this zone's storage
    base_path: PathBuf,
    /// Path to the files subdirectory
    files_path: PathBuf,
    /// Path to metadata JSON file
    metadata_path: PathBuf,
}

impl LocalBackend {
    /// Create a new LocalBackend for the given zone.
    ///
    /// Creates the necessary directories if they don't exist.
    pub fn new(zone_name: &str) -> Result<Self> {
        let home = dirs::home_dir().ok_or(AppError::NoHomeDirectory)?;
        let base_path = home.join(".rxiu").join("zones").join(zone_name);
        let files_path = base_path.join("files");
        let metadata_path = base_path.join("metadata.json");

        fs::create_dir_all(&files_path)?;

        // Initialize empty metadata file if it doesn't exist
        if !metadata_path.exists() {
            let empty: Vec<FileMetadata> = vec![];
            let json = serde_json::to_string_pretty(&empty)?;
            fs::write(&metadata_path, json)?;
        }

        Ok(Self {
            base_path,
            files_path,
            metadata_path,
        })
    }

    /// Load all metadata from disk.
    fn load_metadata(&self) -> Result<Vec<FileMetadata>> {
        let content = fs::read_to_string(&self.metadata_path)?;
        let metadata: Vec<FileMetadata> = serde_json::from_str(&content)?;
        Ok(metadata)
    }

    /// Save all metadata to disk.
    fn save_metadata(&self, metadata: &[FileMetadata]) -> Result<()> {
        let json = serde_json::to_string_pretty(metadata)?;
        fs::write(&self.metadata_path, json)?;
        Ok(())
    }

    /// Compute SHA-256 hash of content.
    fn compute_hash(content: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content);
        let result = hasher.finalize();
        hex::encode(result)
    }

    /// Get the base path for external access (e.g., zone deletion).
    pub fn base_path(&self) -> &PathBuf {
        &self.base_path
    }
}

// Add hex encoding since sha2 outputs raw bytes
mod hex {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        let bytes = bytes.as_ref();
        let mut s = String::with_capacity(bytes.len() * 2);
        for &b in bytes {
            s.push(HEX_CHARS[(b >> 4) as usize] as char);
            s.push(HEX_CHARS[(b & 0xf) as usize] as char);
        }
        s
    }
}

impl StorageBackend for LocalBackend {
    fn store(&self, name: &str, content: &[u8]) -> Result<FileMetadata> {
        let file_path = self.files_path.join(name);

        // Write content to file
        let mut file = File::create(&file_path)?;
        file.write_all(content)?;

        // Create metadata
        let metadata = FileMetadata {
            name: name.to_string(),
            size: content.len() as u64,
            content_hash: Self::compute_hash(content),
            created_at: None, // Could extract from original file if passed
            imported_at: Utc::now(),
        };

        // Update metadata file
        let mut all_metadata = self.load_metadata()?;
        all_metadata.push(metadata.clone());
        self.save_metadata(&all_metadata)?;

        Ok(metadata)
    }

    fn retrieve(&self, name: &str) -> Result<Vec<u8>> {
        let file_path = self.files_path.join(name);

        if !file_path.exists() {
            return Err(AppError::FileNotFound(name.to_string()));
        }

        let mut file = File::open(&file_path)?;
        let mut content = Vec::new();
        file.read_to_end(&mut content)?;

        Ok(content)
    }

    fn read_chunk(&self, name: &str, offset: u64, size: u64) -> Result<Vec<u8>> {
        let file_path = self.files_path.join(name);

        if !file_path.exists() {
            return Err(AppError::FileNotFound(name.to_string()));
        }

        let file_size = fs::metadata(&file_path)?.len();
        if offset >= file_size {
            return Err(AppError::Io(std::io::Error::other(
                "Chunk offset out of range",
            )));
        }

        let to_read = (file_size - offset).min(size) as usize;
        let mut file = File::open(&file_path)?;
        file.seek(SeekFrom::Start(offset))?;

        let mut buf = vec![0u8; to_read];
        let mut read = 0;
        while read < to_read {
            let n = file.read(&mut buf[read..])?;
            if n == 0 {
                break;
            }
            read += n;
        }
        buf.truncate(read);

        Ok(buf)
    }

    fn delete(&self, name: &str) -> Result<()> {
        let file_path = self.files_path.join(name);

        if !file_path.exists() {
            return Err(AppError::FileNotFound(name.to_string()));
        }

        fs::remove_file(&file_path)?;

        // Update metadata
        let mut all_metadata = self.load_metadata()?;
        all_metadata.retain(|m| m.name != name);
        self.save_metadata(&all_metadata)?;

        Ok(())
    }

    fn list(&self) -> Result<Vec<FileMetadata>> {
        self.load_metadata()
    }

    fn exists(&self, name: &str) -> bool {
        self.files_path.join(name).exists()
    }
}
