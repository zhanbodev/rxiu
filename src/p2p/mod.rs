//! P2P networking module using libp2p.
//!
//! Provides LAN discovery via mDNS and file transfer via request-response protocol.

mod codec;
pub mod messages;
pub mod node;
pub mod peer_store;
pub mod protocol;
pub mod recovery;
pub mod service;

pub use messages::{
    FILE_CHUNK_SIZE, FileChunk, FileMeta, FileRequest, FileResponse, RsBlock, RsHave, ZoneRequest,
    ZoneResponse,
};
pub use node::P2PNode;
pub use protocol::PeerEntry;
pub use service::P2PService;
