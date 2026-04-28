//! libp2p node implementation.
//!
//! Handles network behavior, mDNS discovery, and request-response protocol.

use std::collections::HashMap;
use std::time::Duration;

use futures::StreamExt;
use libp2p::{
    Multiaddr, PeerId, StreamProtocol, Swarm, mdns, noise,
    request_response::{self, ProtocolSupport, ResponseChannel},
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux,
};

use super::codec;
use super::messages::{FileChunk, FileMeta, FileRequest, FileResponse, RsBlock, RsHave};
use crate::rs::RsFileEntry;

/// Protocol name for file transfer.
const PROTOCOL_NAME: StreamProtocol = StreamProtocol::new("/rxiu/file/1.0.0");

/// Combined network behavior.
#[derive(NetworkBehaviour)]
pub struct NodeBehaviour {
    /// mDNS for LAN discovery.
    pub mdns: mdns::tokio::Behaviour,
    /// Request-response for file transfer.
    pub file_transfer: codec::Behaviour<FileRequest, FileResponse>,
}

/// Information about a discovered peer.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PeerInfo {
    #[serde(with = "peer_id_serde")]
    pub peer_id: PeerId,
    pub name: String,
    #[serde(with = "multiaddr_vec_serde")]
    pub addrs: Vec<Multiaddr>,
}

mod peer_id_serde {
    use libp2p::PeerId;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(peer_id: &PeerId, s: S) -> Result<S::Ok, S::Error> {
        peer_id.to_string().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<PeerId, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

mod multiaddr_vec_serde {
    use libp2p::Multiaddr;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(addrs: &[Multiaddr], s: S) -> Result<S::Ok, S::Error> {
        let strings: Vec<String> = addrs.iter().map(|a| a.to_string()).collect();
        strings.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Multiaddr>, D::Error> {
        let strings: Vec<String> = Vec::deserialize(d)?;
        strings
            .into_iter()
            .map(|s| s.parse().map_err(serde::de::Error::custom))
            .collect()
    }
}

/// Events from the P2P node.
#[derive(Debug)]
pub enum NodeEvent {
    /// New peer discovered.
    PeerDiscovered(PeerInfo),
    /// Peer expired/disconnected.
    PeerExpired(PeerId),
    /// Heartbeat pong received.
    PongReceived { peer_id: PeerId },
    /// Incoming request from a peer (needs response).
    IncomingRequest {
        peer_id: PeerId,
        request: FileRequest,
        channel: ResponseChannel<FileResponse>,
    },
    /// Response to ListRemoteZones.
    ZonesReceived { peer_id: PeerId, zones: Vec<String> },
    /// Response to ListRemoteFiles.
    FilesReceived {
        peer_id: PeerId,
        zone: String,
        files: Vec<crate::storage::FileMetadata>,
    },
    /// Response to FetchFile.
    FileReceived {
        name: String,
        content: Vec<u8>,
        hash: String,
    },
    /// Response to GetFileMeta.
    FileMetaReceived { meta: FileMeta },
    /// Response to GetFileChunk.
    FileChunkReceived { chunk: FileChunk },
    /// RS: list of shared files.
    RsFilesReceived {
        peer_id: PeerId,
        files: Vec<RsFileEntry>,
    },
    /// RS: file metadata.
    RsMetaReceived { file: RsFileEntry },
    /// RS: block data.
    RsBlockReceived { block: RsBlock },
    /// RS: multiple block data.
    RsBlocksReceived {
        request_id: request_response::OutboundRequestId,
        blocks: Vec<RsBlock>,
    },
    /// RS: block availability.
    RsHaveReceived {
        peer_id: PeerId,
        name: String,
        hashes: Vec<String>,
    },
    /// RS: ack.
    RsOk,
    /// Peer exchange: list of peers from remote.
    PeersReceived {
        from: PeerId,
        peers: Vec<crate::p2p::protocol::PeerEntry>,
    },
    /// Renew: version info received from peer.
    RenewVersionReceived {
        peer_id: PeerId,
        version: crate::renew::VersionInfo,
    },
    /// Renew: binary chunk received from peer.
    RenewChunkReceived {
        peer_id: PeerId,
        offset: u64,
        data: Vec<u8>,
        is_last: bool,
    },
    /// Response-level error.
    ResponseError {
        request_id: request_response::OutboundRequestId,
        message: String,
    },
    /// Error occurred.
    Error(String),
}

/// The libp2p node.
pub struct P2PNode {
    swarm: Swarm<NodeBehaviour>,
    discovered_peers: HashMap<PeerId, PeerInfo>,
    pending_requests: HashMap<request_response::OutboundRequestId, PeerId>,
}

impl P2PNode {
    /// Create a new P2P node.
    pub fn new() -> crate::Result<Self> {
        // Configure TCP with keep-alive to detect dead connections faster
        let tcp_config = tcp::Config::default().nodelay(true); // Disable Nagle's algorithm for lower latency

        let swarm = libp2p::SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(tcp_config, noise::Config::new, yamux::Config::default)
            .map_err(|e| std::io::Error::other(e.to_string()))?
            .with_behaviour(|key| {
                // mDNS for LAN discovery with shorter query interval for faster recovery
                let mut mdns_config = mdns::Config::default();
                mdns_config.query_interval = Duration::from_secs(30); // 30s instead of 5min default
                let mdns = mdns::tokio::Behaviour::new(mdns_config, key.public().to_peer_id())
                    .expect("mDNS should work");

                // Request-response for file transfer with shorter timeout
                let file_transfer = codec::Behaviour::new(
                    [(PROTOCOL_NAME, ProtocolSupport::Full)],
                    request_response::Config::default()
                        .with_request_timeout(Duration::from_secs(30)),
                );

                NodeBehaviour {
                    mdns,
                    file_transfer,
                }
            })
            .map_err(|e| std::io::Error::other(e.to_string()))?
            // Longer idle timeout: 60 seconds to avoid premature disconnection
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();

        Ok(Self {
            swarm,
            discovered_peers: HashMap::new(),
            pending_requests: HashMap::new(),
        })
    }

    /// Get the local peer ID.
    pub fn local_peer_id(&self) -> PeerId {
        *self.swarm.local_peer_id()
    }

    /// Start listening on available interfaces.
    pub fn start_listening(&mut self) -> crate::Result<()> {
        // Listen on all interfaces
        self.swarm
            .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
            .map_err(|e| std::io::Error::other(e.to_string()))?;

        Ok(())
    }

    /// Get list of discovered peers.
    pub fn get_peers(&self) -> Vec<PeerInfo> {
        self.discovered_peers.values().cloned().collect()
    }

    /// Get peer count.
    pub fn peer_count(&self) -> usize {
        self.discovered_peers.len()
    }

    /// Get local listening addresses (for peer exchange).
    pub fn listening_addrs(&self) -> Option<Vec<Multiaddr>> {
        let addrs: Vec<Multiaddr> = self.swarm.listeners().cloned().collect();
        if addrs.is_empty() { None } else { Some(addrs) }
    }

    /// Remove a peer from the discovered list (e.g., after heartbeat failure).
    pub fn remove_peer(&mut self, peer_id: PeerId) {
        self.discovered_peers.remove(&peer_id);
        // Disconnect from the peer
        let _ = self.swarm.disconnect_peer_id(peer_id);
    }

    /// Send a request to a peer.
    pub fn send_request(
        &mut self,
        peer_id: PeerId,
        request: FileRequest,
    ) -> request_response::OutboundRequestId {
        let request_id = self
            .swarm
            .behaviour_mut()
            .file_transfer
            .send_request(&peer_id, request);
        self.pending_requests.insert(request_id, peer_id);
        request_id
    }

    /// Send a response to a pending request.
    pub fn send_response(
        &mut self,
        channel: ResponseChannel<FileResponse>,
        response: FileResponse,
    ) {
        let _ = self
            .swarm
            .behaviour_mut()
            .file_transfer
            .send_response(channel, response);
    }

    /// Dial a peer by address. Used for peer exchange.
    pub fn dial_peer(&mut self, peer_id: PeerId, addr: Multiaddr) {
        // Skip if already known
        if self.discovered_peers.contains_key(&peer_id) {
            return;
        }
        // Add address and dial
        self.swarm.add_peer_address(peer_id, addr.clone());
        if let Err(e) = self.swarm.dial(peer_id) {
            tracing::debug!("[P2P] Failed to dial {}: {}", peer_id, e);
        } else {
            tracing::info!("[P2P] Dialing peer {} at {}", peer_id, addr);
        }
    }

    /// Probe all known peers by sending GetPeers request.
    /// This triggers peer exchange and helps reconnect after sleep/wake.
    /// Returns the number of peers probed.
    pub fn probe_all_peers(&mut self) -> usize {
        let peer_ids: Vec<PeerId> = self.discovered_peers.keys().cloned().collect();
        let count = peer_ids.len();
        for peer_id in peer_ids {
            tracing::info!("[P2P] Probing peer {} for peer exchange", peer_id);
            self.send_request(peer_id, FileRequest::GetPeers);
        }
        count
    }

    /// Process the next swarm event.
    pub async fn next_event(&mut self) -> Option<NodeEvent> {
        loop {
            match self.swarm.select_next_some().await {
                // mDNS events
                SwarmEvent::Behaviour(NodeBehaviourEvent::Mdns(event)) => {
                    match event {
                        mdns::Event::Discovered(peers) => {
                            for (peer_id, addr) in peers {
                                if peer_id == *self.swarm.local_peer_id() {
                                    continue;
                                }

                                // Dial the peer
                                if let Err(e) = self.swarm.dial(addr.clone()) {
                                    tracing::warn!("Failed to dial {}: {}", peer_id, e);
                                    continue;
                                }

                                let info =
                                    self.discovered_peers.entry(peer_id).or_insert_with(|| {
                                        PeerInfo {
                                            peer_id,
                                            name: peer_id.to_string()[..8].to_string(),
                                            addrs: vec![],
                                        }
                                    });

                                if !info.addrs.contains(&addr) {
                                    info.addrs.push(addr);
                                }

                                return Some(NodeEvent::PeerDiscovered(info.clone()));
                            }
                        }
                        mdns::Event::Expired(peers) => {
                            if let Some((peer_id, _)) = peers.into_iter().next() {
                                self.discovered_peers.remove(&peer_id);
                                return Some(NodeEvent::PeerExpired(peer_id));
                            }
                        }
                    }
                }

                // File transfer events
                SwarmEvent::Behaviour(NodeBehaviourEvent::FileTransfer(event)) => {
                    match event {
                        request_response::Event::Message { peer, message } => {
                            match message {
                                request_response::Message::Request {
                                    request, channel, ..
                                } => {
                                    // Pass request to service layer for handling
                                    return Some(NodeEvent::IncomingRequest {
                                        peer_id: peer,
                                        request,
                                        channel,
                                    });
                                }
                                request_response::Message::Response {
                                    request_id,
                                    response,
                                } => {
                                    self.pending_requests.remove(&request_id);
                                    return Some(match response {
                                        FileResponse::Pong => {
                                            // Heartbeat response - notify service layer
                                            NodeEvent::PongReceived { peer_id: peer }
                                        }
                                        FileResponse::Zones(zones) => NodeEvent::ZonesReceived {
                                            peer_id: peer,
                                            zones,
                                        },
                                        FileResponse::Files { zone, files } => {
                                            NodeEvent::FilesReceived {
                                                peer_id: peer,
                                                zone,
                                                files,
                                            }
                                        }
                                        FileResponse::FileData {
                                            name,
                                            content,
                                            hash,
                                        } => NodeEvent::FileReceived {
                                            name,
                                            content,
                                            hash,
                                        },
                                        FileResponse::FileMeta(meta) => {
                                            NodeEvent::FileMetaReceived { meta }
                                        }
                                        FileResponse::FileChunk(chunk) => {
                                            NodeEvent::FileChunkReceived { chunk }
                                        }
                                        FileResponse::RsFiles(files) => {
                                            NodeEvent::RsFilesReceived {
                                                peer_id: peer,
                                                files,
                                            }
                                        }
                                        FileResponse::RsMeta(file) => {
                                            NodeEvent::RsMetaReceived { file }
                                        }
                                        FileResponse::RsBlock(block) => {
                                            NodeEvent::RsBlockReceived { block }
                                        }
                                        FileResponse::RsBlocks(blocks) => {
                                            NodeEvent::RsBlocksReceived { request_id, blocks }
                                        }
                                        FileResponse::RsHave(RsHave { name, hashes }) => {
                                            NodeEvent::RsHaveReceived {
                                                peer_id: peer,
                                                name,
                                                hashes,
                                            }
                                        }
                                        FileResponse::RsOk => NodeEvent::RsOk,
                                        FileResponse::Peers(peers) => {
                                            NodeEvent::PeersReceived { from: peer, peers }
                                        }
                                        FileResponse::Error(msg) => NodeEvent::ResponseError {
                                            request_id,
                                            message: msg,
                                        },
                                        FileResponse::RenewVersion(version) => {
                                            NodeEvent::RenewVersionReceived {
                                                peer_id: peer,
                                                version,
                                            }
                                        }
                                        FileResponse::RenewBinaryChunk {
                                            offset,
                                            data,
                                            is_last,
                                        } => NodeEvent::RenewChunkReceived {
                                            peer_id: peer,
                                            offset,
                                            data,
                                            is_last,
                                        },
                                    });
                                }
                            }
                        }
                        request_response::Event::OutboundFailure {
                            request_id, error, ..
                        } => {
                            self.pending_requests.remove(&request_id);
                            return Some(NodeEvent::Error(format!("Request failed: {}", error)));
                        }
                        request_response::Event::InboundFailure { error, .. } => {
                            tracing::warn!("Inbound request failed: {}", error);
                        }
                        request_response::Event::ResponseSent { .. } => {}
                    }
                }

                // Connection events
                SwarmEvent::ConnectionEstablished {
                    peer_id, endpoint, ..
                } => {
                    tracing::info!("Connected to {}", peer_id);
                    // If this is a new peer (e.g., dialed via peer exchange), add to discovered list
                    if !self.discovered_peers.contains_key(&peer_id) {
                        let addr = endpoint.get_remote_address().clone();
                        let info = PeerInfo {
                            peer_id,
                            name: format!("{}", peer_id).chars().take(12).collect(),
                            addrs: vec![addr],
                        };
                        self.discovered_peers.insert(peer_id, info.clone());
                        return Some(NodeEvent::PeerDiscovered(info));
                    }
                }
                SwarmEvent::ConnectionClosed {
                    peer_id,
                    num_established,
                    ..
                } => {
                    tracing::info!("Disconnected from {}", peer_id);
                    // If no more connections to this peer, remove from discovered list
                    if num_established == 0 {
                        if self.discovered_peers.remove(&peer_id).is_some() {
                            return Some(NodeEvent::PeerExpired(peer_id));
                        }
                    }
                }

                // Other events
                SwarmEvent::NewListenAddr { address, .. } => {
                    tracing::info!("Listening on {}", address);
                }

                _ => {}
            }
        }
    }
}
