//! RS sync helpers shared by TUI and daemon.

use std::collections::HashSet;

use sha2::{Digest, Sha256};

use crate::error::Result;
use crate::rs::{RsBlockEntry, RsFileEntry, RsStore};

pub fn entry_members(entry: &RsFileEntry, fallback: &[String]) -> Vec<String> {
    if !entry.members.is_empty() {
        let mut members = entry.members.clone();
        members.sort();
        members
    } else {
        fallback.to_vec()
    }
}

pub fn hrw_owner_index(block: &RsBlockEntry, members: &[String]) -> Option<usize> {
    if members.is_empty() {
        return None;
    }
    let mut best_idx = None;
    let mut best_score: Option<Vec<u8>> = None;
    for (idx, member) in members.iter().enumerate() {
        let mut hasher = Sha256::new();
        hasher.update(member.as_bytes());
        hasher.update(block.hash.as_bytes());
        let score = hasher.finalize().to_vec();
        if best_score.as_ref().map_or(true, |s| score > *s) {
            best_score = Some(score);
            best_idx = Some(idx);
        }
    }
    best_idx
}

pub fn needs_sync(store: &RsStore, local_id: &str, fallback: &[String]) -> Result<bool> {
    let files = store.list_files()?;
    for file in files {
        let members = entry_members(&file, fallback);
        if members.is_empty() {
            continue;
        }
        let Some(local_index) = members.iter().position(|id| id == local_id) else {
            continue;
        };
        let missing_owned = file
            .blocks
            .iter()
            .filter(|b| hrw_owner_index(b, &members) == Some(local_index))
            .any(|b| !store.has_block(&b.hash));
        if missing_owned {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn prune_unowned_blocks(store: &RsStore, local_id: &str, fallback: &[String]) -> Result<()> {
    let files = store.list_files()?;
    let mut owned_hashes = HashSet::new();
    for file in &files {
        let members = entry_members(file, fallback);
        let Some(local_index) = members.iter().position(|id| id == local_id) else {
            continue;
        };
        for block in &file.blocks {
            if hrw_owner_index(block, &members) == Some(local_index) {
                owned_hashes.insert(block.hash.clone());
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(store.blocks_path()) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if !owned_hashes.contains(name) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
    for mut file in files {
        let members = entry_members(&file, fallback);
        let Some(local_index) = members.iter().position(|id| id == local_id) else {
            file.complete = false;
            file.syncing = false;
            store.upsert_file(file)?;
            continue;
        };
        let complete = file
            .blocks
            .iter()
            .filter(|b| hrw_owner_index(b, &members) == Some(local_index))
            .all(|b| owned_hashes.contains(&b.hash));
        file.complete = complete && !file.blocks.is_empty();
        file.syncing = false;
        store.upsert_file(file)?;
    }
    Ok(())
}
