//! Peer Exchange protocol implementation.
//!
//! Allows nodes to share their known peers with each other,
//! enabling indirect peer discovery beyond mDNS.

use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};

use crate::p2p::node::PeerInfo;

/// Peer entry for serialization over the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerEntry {
    pub peer_id: String,
    pub addrs: Vec<String>,
}

impl PeerEntry {
    /// Create a PeerEntry from a PeerInfo.
    pub fn from_peer_info(info: &PeerInfo) -> Self {
        Self {
            peer_id: info.peer_id.to_string(),
            addrs: info.addrs.iter().map(|a| a.to_string()).collect(),
        }
    }

    /// Convert a list of PeerInfo to PeerEntry.
    pub fn from_peer_list(peers: &[PeerInfo]) -> Vec<Self> {
        peers.iter().map(Self::from_peer_info).collect()
    }

    /// Parse peer_id string to PeerId.
    pub fn parse_peer_id(&self) -> Option<PeerId> {
        self.peer_id.parse().ok()
    }

    /// Parse first valid address.
    pub fn parse_first_addr(&self) -> Option<Multiaddr> {
        for addr_str in &self.addrs {
            if let Ok(addr) = addr_str.parse() {
                return Some(addr);
            }
        }
        None
    }
}

/// Process received peers and dial unknown ones.
pub fn process_received_peers<F>(
    local_peer_id: PeerId,
    peers: Vec<PeerEntry>,
    is_known: impl Fn(&PeerId) -> bool,
    dial_peer: F,
) where
    F: Fn(PeerId, Multiaddr),
{
    for entry in peers {
        // Parse peer_id
        let Some(peer_id) = entry.parse_peer_id() else {
            continue;
        };

        // Skip self
        if peer_id == local_peer_id {
            continue;
        }

        // Skip if already known
        if is_known(&peer_id) {
            continue;
        }

        // Dial first valid address
        if let Some(addr) = entry.parse_first_addr() {
            dial_peer(peer_id, addr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_entry_roundtrip() {
        let entry = PeerEntry {
            peer_id: "12D3KooWTest".to_string(),
            addrs: vec!["/ip4/127.0.0.1/tcp/1234".to_string()],
        };

        assert!(entry.parse_first_addr().is_some());
    }
}
