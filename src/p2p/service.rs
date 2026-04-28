//! P2P service layer.
//!
//! High-level interface for P2P operations, integrating with zone storage.

use std::collections::HashMap;
use std::sync::Arc;

use libp2p::{PeerId, request_response};
use tokio::sync::{RwLock, mpsc, oneshot};

use super::messages::{
    FILE_CHUNK_SIZE, FileChunk, FileMeta, FileRequest, FileResponse, RsBlock, RsHave,
};
use super::node::{NodeEvent, P2PNode, PeerInfo};
use crate::error::AppError;
use crate::rs::{RsFileEntry, RsStore};
use crate::storage::ZoneManager;

/// Commands for the P2P service.
#[derive(Debug)]
pub enum ServiceCommand {
    /// Get list of peers.
    GetPeers {
        resp: oneshot::Sender<Vec<PeerInfo>>,
    },
    /// Get peer count.
    GetPeerCount { resp: oneshot::Sender<usize> },
    /// List zones on a remote peer.
    ListRemoteZones {
        peer_id: PeerId,
        resp: oneshot::Sender<crate::Result<Vec<String>>>,
    },
    /// List files in a remote zone.
    ListRemoteFiles {
        peer_id: PeerId,
        zone: String,
        resp: oneshot::Sender<crate::Result<Vec<crate::storage::FileMetadata>>>,
    },
    /// Fetch a file from a remote peer.
    FetchFile {
        peer_id: PeerId,
        zone: String,
        name: String,
        resp: oneshot::Sender<crate::Result<(Vec<u8>, String)>>,
    },
    /// Fetch file metadata from a remote peer.
    GetFileMeta {
        peer_id: PeerId,
        zone: String,
        name: String,
        resp: oneshot::Sender<crate::Result<FileMeta>>,
    },
    /// Fetch a file chunk from a remote peer.
    GetFileChunk {
        peer_id: PeerId,
        zone: String,
        name: String,
        offset: u64,
        size: u64,
        resp: oneshot::Sender<crate::Result<FileChunk>>,
    },
    /// RS: list files from a peer.
    RsList {
        peer_id: PeerId,
        resp: oneshot::Sender<crate::Result<Vec<RsFileEntry>>>,
    },
    /// RS: announce a file to a peer.
    RsAnnounce { peer_id: PeerId, file: RsFileEntry },
    /// RS: get file metadata.
    RsGetMeta {
        peer_id: PeerId,
        name: String,
        resp: oneshot::Sender<crate::Result<RsFileEntry>>,
    },
    /// RS: get a block by hash.
    RsGetBlock {
        peer_id: PeerId,
        hash: String,
        resp: oneshot::Sender<crate::Result<RsBlock>>,
    },
    /// RS: get multiple blocks by hash.
    RsGetBlocks {
        peer_id: PeerId,
        hashes: Vec<String>,
        resp: oneshot::Sender<crate::Result<Vec<RsBlock>>>,
    },
    /// RS: ask which blocks a peer has for a file.
    RsHave {
        peer_id: PeerId,
        name: String,
        resp: oneshot::Sender<crate::Result<RsHave>>,
    },
    /// RS: delete a file on a peer.
    RsDelete { peer_id: PeerId, name: String },
    /// Refresh LAN peers (trigger network recovery).
    RefreshPeers {
        resp: oneshot::Sender<crate::Result<usize>>,
    },
    /// Renew: get version from a peer.
    RenewGetVersion {
        peer_id: PeerId,
        resp: oneshot::Sender<crate::Result<crate::renew::VersionInfo>>,
    },
    /// Renew: get binary chunk from a peer.
    RenewGetBinaryChunk {
        peer_id: PeerId,
        offset: u64,
        length: u32,
        resp: oneshot::Sender<crate::Result<(Vec<u8>, bool)>>,
    },
}

/// Handle to the P2P service.
#[derive(Clone)]
pub struct P2PService {
    cmd_tx: mpsc::Sender<ServiceCommand>,
    local_peer_id: PeerId,
}

impl P2PService {
    /// Start the P2P service.
    pub async fn start(
        zone_manager: Arc<RwLock<ZoneManager>>,
        rs_store: Arc<RwLock<RsStore>>,
    ) -> crate::Result<Self> {
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<ServiceCommand>(32);

        // Create the node
        let mut node = P2PNode::new()?;
        let local_peer_id = node.local_peer_id();
        tracing::info!("Local peer ID: {}", local_peer_id);

        // Start listening
        node.start_listening()?;

        // Spawn the event loop
        tokio::spawn(async move {
            #[allow(clippy::type_complexity)]
            let mut pending_zone_requests: HashMap<
                PeerId,
                oneshot::Sender<crate::Result<Vec<String>>>,
            > = HashMap::new();
            #[allow(clippy::type_complexity)]
            let mut pending_file_list_requests: HashMap<
                PeerId,
                oneshot::Sender<crate::Result<Vec<crate::storage::FileMetadata>>>,
            > = HashMap::new();
            #[allow(clippy::type_complexity)]
            let mut pending_file_requests: HashMap<
                String,
                oneshot::Sender<crate::Result<(Vec<u8>, String)>>,
            > = HashMap::new();
            #[allow(clippy::type_complexity)]
            let mut pending_file_meta_requests: HashMap<
                String,
                oneshot::Sender<crate::Result<FileMeta>>,
            > = HashMap::new();
            #[allow(clippy::type_complexity)]
            let mut pending_file_chunk_requests: HashMap<
                String,
                oneshot::Sender<crate::Result<FileChunk>>,
            > = HashMap::new();
            #[allow(clippy::type_complexity)]
            let mut pending_rs_list_requests: HashMap<
                PeerId,
                oneshot::Sender<crate::Result<Vec<RsFileEntry>>>,
            > = HashMap::new();
            #[allow(clippy::type_complexity)]
            let mut pending_rs_meta_requests: HashMap<
                String,
                oneshot::Sender<crate::Result<RsFileEntry>>,
            > = HashMap::new();
            #[allow(clippy::type_complexity)]
            let mut pending_rs_block_requests: HashMap<
                String,
                oneshot::Sender<crate::Result<RsBlock>>,
            > = HashMap::new();
            let mut pending_rs_block_ids: HashMap<request_response::OutboundRequestId, String> =
                HashMap::new();
            let mut pending_rs_blocks_requests: HashMap<
                request_response::OutboundRequestId,
                oneshot::Sender<crate::Result<Vec<RsBlock>>>,
            > = HashMap::new();
            #[allow(clippy::type_complexity)]
            let mut pending_rs_have_requests: HashMap<
                String,
                oneshot::Sender<crate::Result<RsHave>>,
            > = HashMap::new();
            let mut pending_rs_have_ids: HashMap<request_response::OutboundRequestId, String> =
                HashMap::new();

            // Renew request tracking
            let mut pending_renew_version_requests: HashMap<
                PeerId,
                oneshot::Sender<crate::Result<crate::renew::VersionInfo>>,
            > = HashMap::new();
            let mut pending_renew_chunk_requests: HashMap<
                String,
                oneshot::Sender<crate::Result<(Vec<u8>, bool)>>,
            > = HashMap::new();

            // Heartbeat tracking: peer_id -> missed ping count
            let mut peer_heartbeat_misses: HashMap<PeerId, u8> = HashMap::new();
            let mut pending_pings: std::collections::HashSet<PeerId> =
                std::collections::HashSet::new();
            let mut heartbeat_interval = tokio::time::interval(std::time::Duration::from_secs(5));

            // Periodic reconnection to persisted peers (every 30 seconds for faster recovery)
            let mut reconnect_interval = tokio::time::interval(std::time::Duration::from_secs(30));

            // Network recovery for wake from sleep scenarios
            let mut network_recovery = super::recovery::NetworkRecovery::new();

            // Load persisted peers and try to reconnect
            if let Ok(persisted_peers) = super::peer_store::load_peers() {
                let count = persisted_peers.len();
                if count > 0 {
                    tracing::info!("[PeerStore] Loading {} persisted peers", count);
                    for p in persisted_peers {
                        if let Ok(peer_id) = p.peer_id.parse::<PeerId>() {
                            // Skip self
                            if peer_id == node.local_peer_id() {
                                continue;
                            }
                            // Try to dial first valid address
                            for addr_str in &p.addrs {
                                if let Ok(addr) = addr_str.parse() {
                                    tracing::info!(
                                        "[PeerStore] Dialing persisted peer {} at {}",
                                        peer_id,
                                        addr_str
                                    );
                                    node.dial_peer(peer_id, addr);
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            loop {
                tokio::select! {
                    // Heartbeat timer
                    _ = heartbeat_interval.tick() => {
                        // First check for wake from sleep (highest priority)
                        if let Some(reason) = network_recovery.detect_wake() {
                            tracing::info!("[NetworkRecovery] Triggering recovery: {}", reason);
                            if let Err(e) = node.start_listening() {
                                tracing::error!("[NetworkRecovery] Failed to restart listener: {}", e);
                            }
                            // Also probe all known peers to trigger peer exchange
                            let probed = node.probe_all_peers();
                            tracing::info!("[NetworkRecovery] Probed {} peers for peer exchange", probed);
                            
                            // CRITICAL: Immediately dial ALL persisted peers after wake
                            // This ensures we reconnect to peers whose mDNS might not be visible yet
                            if let Ok(persisted_peers) = super::peer_store::load_peers() {
                                let local_id = node.local_peer_id();
                                let mut dialed = 0;
                                for p in persisted_peers {
                                    if let Ok(peer_id) = p.peer_id.parse::<PeerId>() {
                                        if peer_id == local_id {
                                            continue;
                                        }
                                        for addr_str in &p.addrs {
                                            if let Ok(addr) = addr_str.parse() {
                                                tracing::info!("[WakeRecovery] Dialing persisted peer {} at {}", peer_id, addr_str);
                                                node.dial_peer(peer_id, addr);
                                                dialed += 1;
                                                break;
                                            }
                                        }
                                    }
                                }
                                if dialed > 0 {
                                    tracing::info!("[WakeRecovery] Dialed {} persisted peers", dialed);
                                }
                            }
                        }

                        // Check for peers that didn't respond to previous ping
                        for peer_id in pending_pings.drain() {
                            let misses = peer_heartbeat_misses.entry(peer_id).or_insert(0);
                            *misses += 1;
                            if *misses >= 4 {
                                // Peer missed 4 heartbeats (20+ seconds), consider dead
                                tracing::info!("[Heartbeat] Peer {} missed {} pings, removing", peer_id, misses);
                                node.remove_peer(peer_id);
                                peer_heartbeat_misses.remove(&peer_id);
                            }
                        }

                        // Send ping to all known peers
                        let current_peer_count = node.peer_count();
                        for peer in node.get_peers() {
                            node.send_request(peer.peer_id, FileRequest::Ping);
                            pending_pings.insert(peer.peer_id);
                        }

                        // Check if network recovery is needed (other strategies)
                        if let Some(reason) = network_recovery.should_recover(current_peer_count) {
                            tracing::info!("[NetworkRecovery] Triggering recovery: {}", reason);
                            if let Err(e) = node.start_listening() {
                                tracing::error!("[NetworkRecovery] Failed to restart listener: {}", e);
                            }
                        }
                    }
                    // Periodic reconnection to persisted peers
                    _ = reconnect_interval.tick() => {
                        if let Ok(persisted_peers) = super::peer_store::load_peers() {
                            let known_peer_ids: std::collections::HashSet<PeerId> =
                                node.get_peers().iter().map(|p| p.peer_id).collect();
                            let local_id = node.local_peer_id();

                            let mut reconnected = 0;
                            for p in persisted_peers {
                                if let Ok(peer_id) = p.peer_id.parse::<PeerId>() {
                                    // Skip self and already connected peers
                                    if peer_id == local_id || known_peer_ids.contains(&peer_id) {
                                        continue;
                                    }
                                    // Try to dial first valid address
                                    for addr_str in &p.addrs {
                                        if let Ok(addr) = addr_str.parse() {
                                            tracing::info!("[PeerStore] Reconnecting to {} at {}", peer_id, addr_str);
                                            node.dial_peer(peer_id, addr);
                                            reconnected += 1;
                                            break;
                                        }
                                    }
                                }
                            }
                            if reconnected > 0 {
                                tracing::info!("[PeerStore] Attempted reconnection to {} offline peers", reconnected);
                            }
                        }
                    }
                    // Handle commands
                    Some(cmd) = cmd_rx.recv() => {
                        match cmd {
                            ServiceCommand::GetPeers { resp } => {
                                let peers: Vec<PeerInfo> = node.get_peers();
                                let _ = resp.send(peers);
                            }
                            ServiceCommand::GetPeerCount { resp } => {
                                let _ = resp.send(node.peer_count());
                            }
                            ServiceCommand::ListRemoteZones { peer_id, resp } => {
                                tracing::info!("[P2P] Sending ListZones to {}", peer_id);
                                node.send_request(peer_id, FileRequest::ListZones);
                                pending_zone_requests.insert(peer_id, resp);
                            }
                            ServiceCommand::ListRemoteFiles { peer_id, zone, resp } => {
                                tracing::info!("[P2P] Sending ListFiles({}) to {}", zone, peer_id);
                                node.send_request(peer_id, FileRequest::ListFiles { zone });
                                pending_file_list_requests.insert(peer_id, resp);
                            }
                            ServiceCommand::FetchFile { peer_id, zone, name, resp } => {
                                let key = format!("{}:{}", zone, name);
                                tracing::debug!("[P2P] Sending GetFile({}/{}) to {}", zone, name, peer_id);
                                node.send_request(peer_id, FileRequest::GetFile { zone, name: name.clone() });
                                pending_file_requests.insert(key, resp);
                            }
                            ServiceCommand::GetFileMeta { peer_id, zone, name, resp } => {
                                let key = format!("{}:{}", zone, name);
                                tracing::debug!("[P2P] Sending GetFileMeta({}/{}) to {}", zone, name, peer_id);
                                node.send_request(peer_id, FileRequest::GetFileMeta { zone, name: name.clone() });
                                pending_file_meta_requests.insert(key, resp);
                            }
                            ServiceCommand::GetFileChunk { peer_id, zone, name, offset, size, resp } => {
                                let key = format!("{}:{}:{}", zone, name, offset);
                                tracing::debug!("[P2P] Sending GetFileChunk({}/{}, offset={}) to {}", zone, name, offset, peer_id);
                                node.send_request(peer_id, FileRequest::GetFileChunk { zone, name: name.clone(), offset, size });
                                pending_file_chunk_requests.insert(key, resp);
                            }
                            ServiceCommand::RsList { peer_id, resp } => {
                                tracing::info!("[P2P] Sending RsList to {}", peer_id);
                                node.send_request(peer_id, FileRequest::RsList);
                                pending_rs_list_requests.insert(peer_id, resp);
                            }
                            ServiceCommand::RsAnnounce { peer_id, file } => {
                                tracing::info!("[P2P] Sending RsAnnounce({}) to {}", file.name, peer_id);
                                node.send_request(peer_id, FileRequest::RsAnnounce { file });
                            }
                            ServiceCommand::RsGetMeta { peer_id, name, resp } => {
                                tracing::info!("[P2P] Sending RsGetMeta({}) to {}", name, peer_id);
                                node.send_request(peer_id, FileRequest::RsGetMeta { name: name.clone() });
                                pending_rs_meta_requests.insert(name, resp);
                            }
                            ServiceCommand::RsGetBlock { peer_id, hash, resp } => {
                                tracing::debug!("[P2P] Sending RsGetBlock({}...) to {}", &hash[..8.min(hash.len())], peer_id);
                                let request_id = node.send_request(peer_id, FileRequest::RsGetBlock { hash: hash.clone() });
                                pending_rs_block_ids.insert(request_id, hash.clone());
                                pending_rs_block_requests.insert(hash, resp);
                            }
                            ServiceCommand::RsGetBlocks { peer_id, hashes, resp } => {
                                tracing::debug!("[P2P] Sending RsGetBlocks({}) to {}", hashes.len(), peer_id);
                                let request_id = node.send_request(peer_id, FileRequest::RsGetBlocks { hashes });
                                pending_rs_blocks_requests.insert(request_id, resp);
                            }
                            ServiceCommand::RsHave { peer_id, name, resp } => {
                                tracing::info!("[P2P] Sending RsHave({}) to {}", name, peer_id);
                                let request_id = node.send_request(peer_id, FileRequest::RsHave { name: name.clone() });
                                let key = format!("{}:{}", peer_id, name);
                                pending_rs_have_ids.insert(request_id, key.clone());
                                pending_rs_have_requests.insert(key, resp);
                            }
                            ServiceCommand::RsDelete { peer_id, name } => {
                                tracing::info!("[P2P] Sending RsDelete({}) to {}", name, peer_id);
                                node.send_request(peer_id, FileRequest::RsDelete { name });
                            }
                            ServiceCommand::RefreshPeers { resp } => {
                                tracing::info!("[P2P] Manual peer refresh triggered");
                                // 1. Restart listener to trigger mDNS rediscovery
                                if let Err(e) = node.start_listening() {
                                    tracing::error!("[P2P] Failed to restart listener: {}", e);
                                }
                                // 2. Probe all known peers to trigger peer exchange
                                // This helps us rediscover the network even if mDNS is slow
                                let probed = node.probe_all_peers();
                                tracing::info!("[P2P] Probed {} known peers for peer exchange", probed);
                                // Return current peer count
                                let count = node.peer_count();
                                let _ = resp.send(Ok(count));
                            }
                            ServiceCommand::RenewGetVersion { peer_id, resp } => {
                                tracing::debug!("[Renew] Sending GetVersion to {}", peer_id);
                                node.send_request(peer_id, FileRequest::RenewGetVersion);
                                pending_renew_version_requests.insert(peer_id, resp);
                            }
                            ServiceCommand::RenewGetBinaryChunk { peer_id, offset, length, resp } => {
                                tracing::debug!("[Renew] Sending GetBinaryChunk to {} (offset={}, length={})", peer_id, offset, length);
                                let key = format!("{}:{}", peer_id, offset);
                                node.send_request(peer_id, FileRequest::RenewGetBinaryChunk { offset, length });
                                pending_renew_chunk_requests.insert(key, resp);
                            }
                        }
                    }

                    // Handle node events
                    Some(event) = node.next_event() => {
                        match event {
                            NodeEvent::PeerDiscovered(info) => {
                                tracing::info!("Discovered peer: {} at {:?}", info.name, info.addrs);
                                // Reset heartbeat tracking for new peer
                                peer_heartbeat_misses.remove(&info.peer_id);
                                // Request peer list for peer exchange
                                node.send_request(info.peer_id, FileRequest::GetPeers);
                                // Persist peer for later reconnection
                                let addrs: Vec<String> = info.addrs.iter().map(|a| a.to_string()).collect();
                                if let Err(e) = super::peer_store::save_peer(&info.peer_id.to_string(), &addrs) {
                                    tracing::warn!("[PeerStore] Failed to save peer: {}", e);
                                }
                            }
                            NodeEvent::PeerExpired(peer_id) => {
                                tracing::info!("Peer expired: {}", peer_id);
                                pending_pings.remove(&peer_id);
                                peer_heartbeat_misses.remove(&peer_id);
                            }
                            NodeEvent::PongReceived { peer_id } => {
                                // Peer responded to heartbeat - it's alive
                                pending_pings.remove(&peer_id);
                                peer_heartbeat_misses.remove(&peer_id);
                            }
                            NodeEvent::IncomingRequest { peer_id, request, channel } => {
                                // Handle GetPeers inline (needs node access)
                                if matches!(request, FileRequest::GetPeers) {
                                    let mut peer_entries: Vec<_> = node.get_peers().iter().map(|p| {
                                        crate::p2p::protocol::PeerEntry {
                                            peer_id: p.peer_id.to_string(),
                                            addrs: p.addrs.iter().map(|a| a.to_string()).collect(),
                                        }
                                    }).collect();
                                    // Also include ourself so the requester can discover us
                                    if let Some(local_addrs) = node.listening_addrs() {
                                        peer_entries.push(crate::p2p::protocol::PeerEntry {
                                            peer_id: node.local_peer_id().to_string(),
                                            addrs: local_addrs.iter().map(|a| a.to_string()).collect(),
                                        });
                                    }
                                    node.send_response(channel, FileResponse::Peers(peer_entries));
                                } else {
                                    let response = handle_incoming_request(&zone_manager, &rs_store, request).await;
                                    tracing::debug!("Responding to request from {}", peer_id);
                                    node.send_response(channel, response);
                                }
                            }
                            NodeEvent::ZonesReceived { peer_id, zones } => {
                                if let Some(resp) = pending_zone_requests.remove(&peer_id) {
                                    let _ = resp.send(Ok(zones));
                                }
                            }
                            NodeEvent::FilesReceived { peer_id, files, .. } => {
                                if let Some(resp) = pending_file_list_requests.remove(&peer_id) {
                                    let _ = resp.send(Ok(files));
                                }
                            }
                            NodeEvent::FileReceived { name, content, hash } => {
                                tracing::debug!("[P2P] FileReceived: {} ({} bytes)", name, content.len());
                                // Find matching request by name suffix
                                let pending_keys: Vec<String> = pending_file_requests.keys().cloned().collect();
                                tracing::trace!("[P2P] Pending keys: {:?}", pending_keys);
                                let key = pending_file_requests.keys()
                                    .find(|k| k.ends_with(&format!(":{}", name)))
                                    .cloned();
                                if let Some(key) = key {
                                    tracing::debug!("[P2P] Matched key: {}", key);
                                    if let Some(resp) = pending_file_requests.remove(&key) {
                                        let _ = resp.send(Ok((content, hash)));
                                    }
                                } else {
                                    tracing::warn!("[P2P] No matching key for file: {}", name);
                                }
                            }
                            NodeEvent::FileMetaReceived { meta } => {
                                let key = format!("{}:{}", meta.zone, meta.name);
                                if let Some(resp) = pending_file_meta_requests.remove(&key) {
                                    let _ = resp.send(Ok(meta));
                                }
                            }
                            NodeEvent::FileChunkReceived { chunk } => {
                                let key = format!("{}:{}:{}", chunk.zone, chunk.name, chunk.offset);
                                if let Some(resp) = pending_file_chunk_requests.remove(&key) {
                                    let _ = resp.send(Ok(chunk));
                                }
                            }
                            NodeEvent::RsFilesReceived { peer_id, files } => {
                                if let Some(resp) = pending_rs_list_requests.remove(&peer_id) {
                                    let _ = resp.send(Ok(files));
                                }
                            }
                            NodeEvent::RsMetaReceived { file } => {
                                let key = file.name.clone();
                                if let Some(resp) = pending_rs_meta_requests.remove(&key) {
                                    let _ = resp.send(Ok(file));
                                }
                            }
                            NodeEvent::RsBlockReceived { block } => {
                                let key = block.hash.clone();
                                if let Some(resp) = pending_rs_block_requests.remove(&key) {
                                    let _ = resp.send(Ok(block));
                                }
                                pending_rs_block_ids.retain(|_, v| v != &key);
                            }
                            NodeEvent::RsBlocksReceived { request_id, blocks } => {
                                if let Some(resp) = pending_rs_blocks_requests.remove(&request_id) {
                                    let _ = resp.send(Ok(blocks));
                                }
                            }
                            NodeEvent::RsHaveReceived { peer_id, name, hashes } => {
                                let key = format!("{}:{}", peer_id, name);
                                if let Some(resp) = pending_rs_have_requests.remove(&key) {
                                    let _ = resp.send(Ok(RsHave { name, hashes }));
                                }
                            }
                            NodeEvent::RsOk => {}
                            NodeEvent::PeersReceived { from, peers } => {
                                tracing::info!("[PeerExchange] Received {} peers from {}", peers.len(), from);
                                let local_id = node.local_peer_id();
                                for entry in peers {
                                    if let (Some(peer_id), Some(addr)) = (entry.parse_peer_id(), entry.parse_first_addr()) {
                                        if peer_id != local_id {
                                            node.dial_peer(peer_id, addr);
                                        }
                                    }
                                }
                            }
                            NodeEvent::RenewVersionReceived { peer_id, version } => {
                                if let Some(resp) = pending_renew_version_requests.remove(&peer_id) {
                                    let _ = resp.send(Ok(version));
                                }
                            }
                            NodeEvent::RenewChunkReceived { peer_id, offset, data, is_last } => {
                                let key = format!("{}:{}", peer_id, offset);
                                if let Some(resp) = pending_renew_chunk_requests.remove(&key) {
                                    let _ = resp.send(Ok((data, is_last)));
                                }
                            }
                            NodeEvent::ResponseError { request_id, message } => {
                                if let Some(hash) = pending_rs_block_ids.remove(&request_id) {
                                    if let Some(resp) = pending_rs_block_requests.remove(&hash) {
                                        let _ = resp.send(Err(AppError::Io(std::io::Error::other(message))));
                                    }
                                } else if let Some(resp) = pending_rs_blocks_requests.remove(&request_id) {
                                    let _ = resp.send(Err(AppError::Io(std::io::Error::other(message))));
                                } else if let Some(key) = pending_rs_have_ids.remove(&request_id) {
                                    if let Some(resp) = pending_rs_have_requests.remove(&key) {
                                        let _ = resp.send(Err(AppError::Io(std::io::Error::other(message))));
                                    }
                                } else {
                                    tracing::warn!("P2P response error: {}", message);
                                }
                            }
                            NodeEvent::Error(msg) => {
                                tracing::error!("P2P error: {}", msg);
                                for (_, resp) in pending_zone_requests.drain() {
                                    let _ = resp.send(Err(AppError::Io(std::io::Error::other(msg.clone()))));
                                }
                                for (_, resp) in pending_file_list_requests.drain() {
                                    let _ = resp.send(Err(AppError::Io(std::io::Error::other(msg.clone()))));
                                }
                                for (_, resp) in pending_file_requests.drain() {
                                    let _ = resp.send(Err(AppError::Io(std::io::Error::other(msg.clone()))));
                                }
                                for (_, resp) in pending_file_meta_requests.drain() {
                                    let _ = resp.send(Err(AppError::Io(std::io::Error::other(msg.clone()))));
                                }
                                for (_, resp) in pending_file_chunk_requests.drain() {
                                    let _ = resp.send(Err(AppError::Io(std::io::Error::other(msg.clone()))));
                                }
                                for (_, resp) in pending_rs_list_requests.drain() {
                                    let _ = resp.send(Err(AppError::Io(std::io::Error::other(msg.clone()))));
                                }
                                for (_, resp) in pending_rs_meta_requests.drain() {
                                    let _ = resp.send(Err(AppError::Io(std::io::Error::other(msg.clone()))));
                                }
                                for (_, resp) in pending_rs_block_requests.drain() {
                                    let _ = resp.send(Err(AppError::Io(std::io::Error::other(msg.clone()))));
                                }
                                for (_, resp) in pending_rs_blocks_requests.drain() {
                                    let _ = resp.send(Err(AppError::Io(std::io::Error::other(msg.clone()))));
                                }
                                for (_, resp) in pending_rs_have_requests.drain() {
                                    let _ = resp.send(Err(AppError::Io(std::io::Error::other(msg.clone()))));
                                }
                                pending_rs_block_ids.clear();
                                pending_rs_have_ids.clear();
                            }
                        }
                    }
                }
            }
        });

        Ok(Self {
            cmd_tx,
            local_peer_id,
        })
    }

    /// Get discovered peers.
    pub async fn get_peers(&self) -> Vec<PeerInfo> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .cmd_tx
            .send(ServiceCommand::GetPeers { resp: tx })
            .await;
        rx.await.unwrap_or_default()
    }

    pub fn local_peer_id(&self) -> PeerId {
        self.local_peer_id
    }

    /// Get peer count.
    pub async fn peer_count(&self) -> usize {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .cmd_tx
            .send(ServiceCommand::GetPeerCount { resp: tx })
            .await;
        rx.await.unwrap_or(0)
    }

    /// List zones on a remote peer (with 10s timeout).
    pub async fn list_remote_zones(&self, peer_id: PeerId) -> crate::Result<Vec<String>> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(ServiceCommand::ListRemoteZones { peer_id, resp: tx })
            .await
            .map_err(|_| AppError::Io(std::io::Error::other("Channel closed")))?;

        // Add timeout
        match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
            Ok(result) => result
                .map_err(|_| AppError::Io(std::io::Error::other("Response channel closed")))?,
            Err(_) => Err(AppError::Io(std::io::Error::other(
                "Request timed out (peer may be offline)",
            ))),
        }
    }

    /// List files in a remote zone (with 10s timeout).
    pub async fn list_remote_files(
        &self,
        peer_id: PeerId,
        zone: &str,
    ) -> crate::Result<Vec<crate::storage::FileMetadata>> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(ServiceCommand::ListRemoteFiles {
                peer_id,
                zone: zone.to_string(),
                resp: tx,
            })
            .await
            .map_err(|_| AppError::Io(std::io::Error::other("Channel closed")))?;

        // Add timeout
        match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
            Ok(result) => result
                .map_err(|_| AppError::Io(std::io::Error::other("Response channel closed")))?,
            Err(_) => Err(AppError::Io(std::io::Error::other(
                "Request timed out (peer may be offline)",
            ))),
        }
    }

    /// Fetch a file from a remote peer (with 60s timeout for large files).
    pub async fn fetch_file(
        &self,
        peer_id: PeerId,
        zone: &str,
        name: &str,
    ) -> crate::Result<(Vec<u8>, String)> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(ServiceCommand::FetchFile {
                peer_id,
                zone: zone.to_string(),
                name: name.to_string(),
                resp: tx,
            })
            .await
            .map_err(|_| AppError::Io(std::io::Error::other("Channel closed")))?;

        // Longer timeout for file downloads
        match tokio::time::timeout(std::time::Duration::from_secs(300), rx).await {
            Ok(result) => result
                .map_err(|_| AppError::Io(std::io::Error::other("Response channel closed")))?,
            Err(_) => Err(AppError::Io(std::io::Error::other(
                "Download timed out (5 minutes)",
            ))),
        }
    }

    /// Fetch file metadata from a remote peer.
    pub async fn get_file_meta(
        &self,
        peer_id: PeerId,
        zone: &str,
        name: &str,
    ) -> crate::Result<FileMeta> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(ServiceCommand::GetFileMeta {
                peer_id,
                zone: zone.to_string(),
                name: name.to_string(),
                resp: tx,
            })
            .await
            .map_err(|_| AppError::Io(std::io::Error::other("Channel closed")))?;

        match tokio::time::timeout(std::time::Duration::from_secs(10), rx).await {
            Ok(result) => result
                .map_err(|_| AppError::Io(std::io::Error::other("Response channel closed")))?,
            Err(_) => Err(AppError::Io(std::io::Error::other(
                "Request timed out (peer may be offline)",
            ))),
        }
    }

    /// Fetch a file chunk from a remote peer.
    pub async fn get_file_chunk(
        &self,
        peer_id: PeerId,
        zone: &str,
        name: &str,
        offset: u64,
        size: u64,
    ) -> crate::Result<FileChunk> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(ServiceCommand::GetFileChunk {
                peer_id,
                zone: zone.to_string(),
                name: name.to_string(),
                offset,
                size,
                resp: tx,
            })
            .await
            .map_err(|_| AppError::Io(std::io::Error::other("Channel closed")))?;

        match tokio::time::timeout(std::time::Duration::from_secs(60), rx).await {
            Ok(result) => result
                .map_err(|_| AppError::Io(std::io::Error::other("Response channel closed")))?,
            Err(_) => Err(AppError::Io(std::io::Error::other(
                "Chunk request timed out",
            ))),
        }
    }

    /// RS: list files from a peer.
    pub async fn rs_list(&self, peer_id: PeerId) -> crate::Result<Vec<RsFileEntry>> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(ServiceCommand::RsList { peer_id, resp: tx })
            .await
            .map_err(|_| AppError::Io(std::io::Error::other("Channel closed")))?;

        match tokio::time::timeout(std::time::Duration::from_secs(10), rx).await {
            Ok(result) => result
                .map_err(|_| AppError::Io(std::io::Error::other("Response channel closed")))?,
            Err(_) => Err(AppError::Io(std::io::Error::other("RS list timed out"))),
        }
    }

    /// RS: announce a file to a peer.
    pub async fn rs_announce(&self, peer_id: PeerId, file: RsFileEntry) -> crate::Result<()> {
        self.cmd_tx
            .send(ServiceCommand::RsAnnounce { peer_id, file })
            .await
            .map_err(|_| AppError::Io(std::io::Error::other("Channel closed")))?;
        Ok(())
    }

    /// RS: get file metadata from a peer.
    pub async fn rs_get_meta(&self, peer_id: PeerId, name: &str) -> crate::Result<RsFileEntry> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(ServiceCommand::RsGetMeta {
                peer_id,
                name: name.to_string(),
                resp: tx,
            })
            .await
            .map_err(|_| AppError::Io(std::io::Error::other("Channel closed")))?;
        match tokio::time::timeout(std::time::Duration::from_secs(10), rx).await {
            Ok(result) => result
                .map_err(|_| AppError::Io(std::io::Error::other("Response channel closed")))?,
            Err(_) => Err(AppError::Io(std::io::Error::other("RS meta timed out"))),
        }
    }

    /// RS: get multiple blocks by hash from a peer.
    pub async fn rs_get_blocks(
        &self,
        peer_id: PeerId,
        hashes: Vec<String>,
    ) -> crate::Result<Vec<RsBlock>> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(ServiceCommand::RsGetBlocks {
                peer_id,
                hashes,
                resp: tx,
            })
            .await
            .map_err(|_| AppError::Io(std::io::Error::other("Channel closed")))?;
        match tokio::time::timeout(std::time::Duration::from_secs(120), rx).await {
            Ok(result) => result
                .map_err(|_| AppError::Io(std::io::Error::other("Response channel closed")))?,
            Err(_) => Err(AppError::Io(std::io::Error::other("RS blocks timed out"))),
        }
    }

    /// RS: get a block by hash from a peer.
    pub async fn rs_get_block(&self, peer_id: PeerId, hash: &str) -> crate::Result<RsBlock> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(ServiceCommand::RsGetBlock {
                peer_id,
                hash: hash.to_string(),
                resp: tx,
            })
            .await
            .map_err(|_| AppError::Io(std::io::Error::other("Channel closed")))?;
        match tokio::time::timeout(std::time::Duration::from_secs(120), rx).await {
            Ok(result) => result
                .map_err(|_| AppError::Io(std::io::Error::other("Response channel closed")))?,
            Err(_) => Err(AppError::Io(std::io::Error::other("RS block timed out"))),
        }
    }

    /// RS: ask which blocks a peer has for a file.
    pub async fn rs_have(&self, peer_id: PeerId, name: &str) -> crate::Result<RsHave> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(ServiceCommand::RsHave {
                peer_id,
                name: name.to_string(),
                resp: tx,
            })
            .await
            .map_err(|_| AppError::Io(std::io::Error::other("Channel closed")))?;
        match tokio::time::timeout(std::time::Duration::from_secs(10), rx).await {
            Ok(result) => result
                .map_err(|_| AppError::Io(std::io::Error::other("Response channel closed")))?,
            Err(_) => Err(AppError::Io(std::io::Error::other("RS have timed out"))),
        }
    }

    /// RS: delete a file on a peer.
    pub async fn rs_delete(&self, peer_id: PeerId, name: &str) -> crate::Result<()> {
        self.cmd_tx
            .send(ServiceCommand::RsDelete {
                peer_id,
                name: name.to_string(),
            })
            .await
            .map_err(|_| AppError::Io(std::io::Error::other("Channel closed")))?;
        Ok(())
    }

    /// Refresh LAN peers by restarting the listener (triggers mDNS rediscovery).
    pub async fn refresh_peers(&self) -> crate::Result<usize> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(ServiceCommand::RefreshPeers { resp: tx })
            .await
            .map_err(|_| AppError::Io(std::io::Error::other("Channel closed")))?;
        match tokio::time::timeout(std::time::Duration::from_secs(5), rx).await {
            Ok(result) => result
                .map_err(|_| AppError::Io(std::io::Error::other("Response channel closed")))?,
            Err(_) => Err(AppError::Io(std::io::Error::other("Refresh timed out"))),
        }
    }

    /// Renew: get version info from a peer.
    pub async fn renew_get_version(
        &self,
        peer_id: PeerId,
    ) -> crate::Result<crate::renew::VersionInfo> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(ServiceCommand::RenewGetVersion { peer_id, resp: tx })
            .await
            .map_err(|_| AppError::Io(std::io::Error::other("Channel closed")))?;
        match tokio::time::timeout(std::time::Duration::from_secs(10), rx).await {
            Ok(result) => result
                .map_err(|_| AppError::Io(std::io::Error::other("Response channel closed")))?,
            Err(_) => Err(AppError::Io(std::io::Error::other(
                "Renew version timed out",
            ))),
        }
    }

    /// Renew: get a binary chunk from a peer.
    pub async fn renew_get_binary_chunk(
        &self,
        peer_id: PeerId,
        offset: u64,
        length: u32,
    ) -> crate::Result<(Vec<u8>, bool)> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(ServiceCommand::RenewGetBinaryChunk {
                peer_id,
                offset,
                length,
                resp: tx,
            })
            .await
            .map_err(|_| AppError::Io(std::io::Error::other("Channel closed")))?;
        match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
            Ok(result) => result
                .map_err(|_| AppError::Io(std::io::Error::other("Response channel closed")))?,
            Err(_) => Err(AppError::Io(std::io::Error::other(
                "Renew binary chunk timed out",
            ))),
        }
    }

    /// Parse a peer ID from string.
    pub fn parse_peer_id(s: &str) -> Option<PeerId> {
        s.parse().ok()
    }
}

async fn handle_incoming_request(
    zone_manager: &Arc<RwLock<ZoneManager>>,
    rs_store: &Arc<RwLock<RsStore>>,
    request: FileRequest,
) -> FileResponse {
    match request {
        FileRequest::Ping => FileResponse::Pong,
        FileRequest::ListZones => {
            let manager = zone_manager.read().await;
            let zones: Vec<String> = manager.list_zones().iter().map(|s| s.to_string()).collect();
            FileResponse::Zones(zones)
        }
        FileRequest::ListFiles { zone } => {
            let manager = zone_manager.read().await;
            // Try to get the zone and list files
            match manager.get_zone(&zone) {
                Some(z) => match z.list() {
                    Ok(files) => FileResponse::Files { zone, files },
                    Err(e) => FileResponse::Error(format!("Failed to list files: {}", e)),
                },
                None => FileResponse::Error(format!("Zone '{}' not found", zone)),
            }
        }
        FileRequest::GetFile { zone, name } => {
            let manager = zone_manager.read().await;
            match manager.get_zone(&zone) {
                Some(z) => {
                    match z.retrieve(&name) {
                        Ok(content) => {
                            // Compute hash
                            use sha2::{Digest, Sha256};
                            let mut hasher = Sha256::new();
                            hasher.update(&content);
                            let hash = format!("{:x}", hasher.finalize());

                            FileResponse::FileData {
                                name,
                                content,
                                hash,
                            }
                        }
                        Err(e) => FileResponse::Error(format!("Failed to get file: {}", e)),
                    }
                }
                None => FileResponse::Error(format!("Zone '{}' not found", zone)),
            }
        }
        FileRequest::GetFileMeta { zone, name } => {
            let manager = zone_manager.read().await;
            match manager.get_zone(&zone) {
                Some(z) => match z.list() {
                    Ok(files) => match files.iter().find(|f| f.name == name) {
                        Some(meta) => {
                            let chunks = if meta.size == 0 {
                                0
                            } else {
                                (meta.size + FILE_CHUNK_SIZE - 1) / FILE_CHUNK_SIZE
                            };
                            FileResponse::FileMeta(FileMeta {
                                zone,
                                name: meta.name.clone(),
                                size: meta.size,
                                hash: meta.content_hash.clone(),
                                chunk_size: FILE_CHUNK_SIZE,
                                chunks,
                            })
                        }
                        None => FileResponse::Error(format!("File '{}' not found", name)),
                    },
                    Err(e) => FileResponse::Error(format!("Failed to list files: {}", e)),
                },
                None => FileResponse::Error(format!("Zone '{}' not found", zone)),
            }
        }
        FileRequest::GetFileChunk {
            zone,
            name,
            offset,
            size,
        } => {
            let manager = zone_manager.read().await;
            match manager.get_zone(&zone) {
                Some(z) => match z.read_chunk(&name, offset, size) {
                    Ok(data) => {
                        use sha2::{Digest, Sha256};
                        let mut hasher = Sha256::new();
                        hasher.update(&data);
                        let hash = format!("{:x}", hasher.finalize());
                        FileResponse::FileChunk(FileChunk {
                            zone,
                            name,
                            offset,
                            data,
                            hash,
                        })
                    }
                    Err(e) => FileResponse::Error(format!("Failed to read chunk: {}", e)),
                },
                None => FileResponse::Error(format!("Zone '{}' not found", zone)),
            }
        }
        FileRequest::RsList => {
            let store = rs_store.read().await;
            match store.list_files() {
                Ok(files) => FileResponse::RsFiles(files),
                Err(e) => FileResponse::Error(format!("Failed to list RS files: {}", e)),
            }
        }
        FileRequest::RsAnnounce { file } => {
            let store = rs_store.write().await;
            match store.apply_remote_meta(file) {
                Ok(()) => FileResponse::RsOk,
                Err(e) => FileResponse::Error(format!("Failed to apply RS meta: {}", e)),
            }
        }
        FileRequest::RsGetMeta { name } => {
            let store = rs_store.read().await;
            match store.get_file(&name) {
                Ok(Some(file)) => FileResponse::RsMeta(file),
                Ok(None) => FileResponse::Error(format!("RS file '{}' not found", name)),
                Err(e) => FileResponse::Error(format!("Failed to get RS meta: {}", e)),
            }
        }
        FileRequest::RsGetBlock { hash } => {
            let store = rs_store.read().await;
            match store.read_block(&hash) {
                Ok(data) => FileResponse::RsBlock(RsBlock { hash, data }),
                Err(e) => FileResponse::Error(format!("Failed to read RS block: {}", e)),
            }
        }
        FileRequest::RsGetBlocks { hashes } => {
            let store = rs_store.read().await;
            let mut blocks = Vec::with_capacity(hashes.len());
            for hash in hashes {
                match store.read_block(&hash) {
                    Ok(data) => blocks.push(RsBlock { hash, data }),
                    Err(e) => {
                        return FileResponse::Error(format!("Failed to read RS block: {}", e));
                    }
                }
            }
            FileResponse::RsBlocks(blocks)
        }
        FileRequest::RsHave { name } => {
            let store = rs_store.read().await;
            match store.file_block_hashes(&name) {
                Ok(hashes) => FileResponse::RsHave(RsHave { name, hashes }),
                Err(e) => {
                    FileResponse::Error(format!("Failed to read RS block availability: {}", e))
                }
            }
        }
        FileRequest::RsDelete { name } => {
            let store = rs_store.write().await;
            match store.remove_file(&name) {
                Ok(()) => FileResponse::RsOk,
                Err(e) => FileResponse::Error(format!("Failed to delete RS file: {}", e)),
            }
        }
        FileRequest::GetPeers => {
            // This is handled inline in the service loop, not here
            FileResponse::Error("GetPeers should be handled inline".to_string())
        }
        FileRequest::RenewGetVersion => match crate::renew::VersionInfo::current() {
            Ok(info) => FileResponse::RenewVersion(info),
            Err(e) => FileResponse::Error(format!("Failed to get version: {}", e)),
        },
        FileRequest::RenewGetBinaryChunk { offset, length } => {
            match get_binary_chunk(offset, length) {
                Ok((data, is_last)) => FileResponse::RenewBinaryChunk {
                    offset,
                    data,
                    is_last,
                },
                Err(e) => FileResponse::Error(format!("Failed to get binary chunk: {}", e)),
            }
        }
    }
}

/// Read a chunk of the current binary for renew protocol.
fn get_binary_chunk(offset: u64, length: u32) -> crate::Result<(Vec<u8>, bool)> {
    use std::io::{Read, Seek, SeekFrom};

    let exe_path = std::env::current_exe()?;
    let mut file = std::fs::File::open(&exe_path)?;
    let file_size = file.metadata()?.len();

    if offset >= file_size {
        return Ok((Vec::new(), true));
    }

    file.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; length as usize];
    let n = file.read(&mut buf)?;
    buf.truncate(n);

    let is_last = offset + n as u64 >= file_size;
    Ok((buf, is_last))
}
