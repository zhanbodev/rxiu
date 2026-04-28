//! RS (Block Sharing) storage layer.
//!
//! Stores file blocks and metadata under ~/.rxiu/rs.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{AppError, Result};

pub mod sync;

const RS_DIR_NAME: &str = "rs";
const RS_BLOCKS_DIR: &str = "blocks";
const RS_METADATA_FILE: &str = "metadata.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RsBlockEntry {
    pub hash: String,
    pub size: u64,
    pub offset: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RsFileEntry {
    pub name: String,
    pub size: u64,
    pub hash: String,
    pub blocks: Vec<RsBlockEntry>,
    #[serde(default)]
    pub members: Vec<String>,
    #[serde(default)]
    pub algo: String,
    pub complete: bool,
    pub syncing: bool,
    pub deletable: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RsMetadata {
    files: Vec<RsFileEntry>,
}

pub struct RsStore {
    base_path: PathBuf,
    blocks_path: PathBuf,
    metadata_path: PathBuf,
}

impl RsStore {
    pub fn new() -> Result<Self> {
        let home = dirs::home_dir().ok_or(AppError::NoHomeDirectory)?;
        let base_path = home.join(".rxiu").join(RS_DIR_NAME);
        let blocks_path = base_path.join(RS_BLOCKS_DIR);
        let metadata_path = base_path.join(RS_METADATA_FILE);

        fs::create_dir_all(&blocks_path)?;
        if !metadata_path.exists() {
            let empty = RsMetadata::default();
            fs::write(&metadata_path, serde_json::to_string_pretty(&empty)?)?;
        }

        Ok(Self {
            base_path,
            blocks_path,
            metadata_path,
        })
    }

    pub fn base_path(&self) -> &Path {
        &self.base_path
    }

    pub fn blocks_path(&self) -> &Path {
        &self.blocks_path
    }

    pub fn metadata_path(&self) -> &Path {
        &self.metadata_path
    }

    pub fn list_files(&self) -> Result<Vec<RsFileEntry>> {
        Ok(self.load_metadata()?.files)
    }

    pub fn get_file(&self, name: &str) -> Result<Option<RsFileEntry>> {
        Ok(self
            .load_metadata()?
            .files
            .into_iter()
            .find(|f| f.name == name))
    }

    pub fn upsert_file(&self, entry: RsFileEntry) -> Result<()> {
        let mut metadata = self.load_metadata()?;
        if let Some(existing) = metadata.files.iter_mut().find(|f| f.name == entry.name) {
            *existing = entry;
        } else {
            metadata.files.push(entry);
        }
        self.save_metadata(&metadata)
    }

    pub fn remove_file(&self, name: &str) -> Result<()> {
        let mut metadata = self.load_metadata()?;
        let removed = metadata.files.iter().find(|f| f.name == name).cloned();
        metadata.files.retain(|f| f.name != name);
        self.save_metadata(&metadata)?;

        if let Some(entry) = removed {
            self.cleanup_blocks(&metadata.files, &entry.blocks)?;
        }
        Ok(())
    }

    pub fn has_block(&self, hash: &str) -> bool {
        self.blocks_path.join(hash).exists()
    }

    pub fn file_block_hashes(&self, name: &str) -> Result<Vec<String>> {
        let entry = self
            .get_file(name)?
            .ok_or_else(|| AppError::Io(std::io::Error::other("RS file not found")))?;
        let mut hashes = Vec::new();
        for block in entry.blocks {
            if self.has_block(&block.hash) {
                hashes.push(block.hash);
            }
        }
        Ok(hashes)
    }

    pub fn read_block(&self, hash: &str) -> Result<Vec<u8>> {
        let path = self.blocks_path.join(hash);
        if !path.exists() {
            return Err(AppError::Io(std::io::Error::other("Block not found")));
        }
        Ok(fs::read(path)?)
    }

    pub fn put_file_from_path(
        &self,
        src: &Path,
        target_name: &str,
        members: Vec<String>,
        algo: &str,
        block_size: u64,
    ) -> Result<RsFileEntry> {
        if !src.exists() {
            return Err(AppError::PathNotFound(src.to_path_buf()));
        }

        let mut file = File::open(src)?;
        let mut offset = 0u64;
        let mut blocks = Vec::new();
        let mut file_hasher = Sha256::new();

        let mut buf = vec![0u8; block_size as usize];
        loop {
            let read = file.read(&mut buf)?;
            if read == 0 {
                break;
            }
            let data = &buf[..read];
            file_hasher.update(data);
            let block_hash = sha256_bytes(data);
            let block_path = self.blocks_path.join(&block_hash);
            if !block_path.exists() {
                fs::write(&block_path, data)?;
            }
            blocks.push(RsBlockEntry {
                hash: block_hash,
                size: read as u64,
                offset,
            });
            offset += read as u64;
        }

        let file_hash = format!("{:x}", file_hasher.finalize());
        let entry = RsFileEntry {
            name: target_name.to_string(),
            size: offset,
            hash: file_hash,
            blocks,
            members,
            algo: algo.to_string(),
            complete: true,
            syncing: false,
            deletable: true,
        };

        self.upsert_file(entry.clone())?;
        Ok(entry)
    }

    pub fn reconstruct_to_path(&self, entry: &RsFileEntry, dest: &Path) -> Result<()> {
        let mut out = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(dest)?;

        let mut file_hasher = Sha256::new();
        let mut blocks = entry.blocks.clone();
        blocks.sort_by_key(|b| b.offset);
        for block in &blocks {
            let data = self.read_block(&block.hash)?;
            file_hasher.update(&data);
            out.write_all(&data)?;
        }
        out.flush()?;

        let hash = format!("{:x}", file_hasher.finalize());
        if hash != entry.hash {
            return Err(AppError::Io(std::io::Error::other("RS file hash mismatch")));
        }

        Ok(())
    }

    pub fn apply_remote_meta(&self, mut entry: RsFileEntry) -> Result<()> {
        let local_blocks = self.local_block_set();
        let missing = entry
            .blocks
            .iter()
            .filter(|b| !local_blocks.contains(&b.hash))
            .count();
        entry.complete = missing == 0;
        entry.syncing = false;
        entry.deletable = true;
        self.upsert_file(entry)
    }

    pub fn write_block(&self, hash: &str, data: &[u8]) -> Result<()> {
        let block_path = self.blocks_path.join(hash);
        if block_path.exists() {
            return Ok(());
        }
        fs::write(block_path, data)?;
        Ok(())
    }

    pub fn verify_and_write_block(&self, hash: &str, data: &[u8]) -> Result<()> {
        if sha256_bytes(data) != hash {
            return Err(AppError::Io(std::io::Error::other("Block hash mismatch")));
        }
        self.write_block(hash, data)
    }

    pub fn file_progress(&self, entry: &RsFileEntry) -> (u64, u64) {
        let local_blocks = self.local_block_set();
        let mut have = 0u64;
        for block in &entry.blocks {
            if local_blocks.contains(&block.hash) {
                have += block.size;
            }
        }
        (have, entry.size)
    }

    pub fn set_syncing(&self, name: &str, syncing: bool) -> Result<()> {
        let mut metadata = self.load_metadata()?;
        if let Some(file) = metadata.files.iter_mut().find(|f| f.name == name) {
            file.syncing = syncing;
            if syncing {
                file.complete = false;
            }
            self.save_metadata(&metadata)?;
        }
        Ok(())
    }

    fn load_metadata(&self) -> Result<RsMetadata> {
        let content = fs::read_to_string(&self.metadata_path)?;
        Ok(serde_json::from_str(&content).unwrap_or_default())
    }

    fn save_metadata(&self, metadata: &RsMetadata) -> Result<()> {
        fs::write(&self.metadata_path, serde_json::to_string_pretty(metadata)?)?;
        Ok(())
    }

    fn cleanup_blocks(
        &self,
        remaining: &[RsFileEntry],
        removed_blocks: &[RsBlockEntry],
    ) -> Result<()> {
        let mut referenced: HashSet<String> = HashSet::new();
        for file in remaining {
            for block in &file.blocks {
                referenced.insert(block.hash.clone());
            }
        }
        for block in removed_blocks {
            if !referenced.contains(&block.hash) {
                let path = self.blocks_path.join(&block.hash);
                let _ = fs::remove_file(path);
            }
        }
        Ok(())
    }

    fn local_block_set(&self) -> HashSet<String> {
        let mut set = HashSet::new();
        if let Ok(entries) = fs::read_dir(&self.blocks_path) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    set.insert(name.to_string());
                }
            }
        }
        set
    }
}

fn sha256_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}
