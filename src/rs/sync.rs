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

/// Get the top-N owner indices for a block using HRW (Highest Random Weight).
/// Returns indices sorted by their HRW score (highest first).
pub fn hrw_top_n_owners(block: &RsBlockEntry, members: &[String], n: usize) -> Vec<usize> {
    if members.is_empty() || n == 0 {
        return Vec::new();
    }

    // Calculate scores for all members
    let mut scored: Vec<(usize, Vec<u8>)> = members
        .iter()
        .enumerate()
        .map(|(idx, member)| {
            let mut hasher = Sha256::new();
            hasher.update(member.as_bytes());
            hasher.update(block.hash.as_bytes());
            let score = hasher.finalize().to_vec();
            (idx, score)
        })
        .collect();

    // Sort by score descending (highest first)
    scored.sort_by(|a, b| b.1.cmp(&a.1));

    // Return top-N indices
    scored.into_iter().take(n).map(|(idx, _)| idx).collect()
}

/// Check if a local node should store this block based on replication factor.
pub fn is_block_assigned_to(
    block: &RsBlockEntry,
    members: &[String],
    local_index: usize,
    replication_factor: usize,
) -> bool {
    let owners = hrw_top_n_owners(block, members, replication_factor);
    owners.contains(&local_index)
}

pub fn needs_sync(
    store: &RsStore,
    local_id: &str,
    fallback: &[String],
    replication_factor: usize,
) -> Result<bool> {
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
            .filter(|b| is_block_assigned_to(b, &members, local_index, replication_factor))
            .any(|b| !store.has_block(&b.hash));
        if missing_owned {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn prune_unowned_blocks(
    store: &RsStore,
    local_id: &str,
    fallback: &[String],
    replication_factor: usize,
) -> Result<()> {
    let files = store.list_files()?;
    let mut owned_hashes = HashSet::new();
    for file in &files {
        let members = entry_members(file, fallback);
        let Some(local_index) = members.iter().position(|id| id == local_id) else {
            continue;
        };
        for block in &file.blocks {
            if is_block_assigned_to(block, &members, local_index, replication_factor) {
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
            .filter(|b| is_block_assigned_to(b, &members, local_index, replication_factor))
            .all(|b| owned_hashes.contains(&b.hash));
        file.complete = complete && !file.blocks.is_empty();
        file.syncing = false;
        store.upsert_file(file)?;
    }
    Ok(())
}
