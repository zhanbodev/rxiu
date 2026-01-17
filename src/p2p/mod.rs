//! P2P networking module using libp2p.
//!
//! Provides LAN discovery via mDNS and file transfer via request-response protocol.

mod codec;
pub mod node;
pub mod messages;
pub mod protocol;
pub mod recovery;
pub mod service;

pub use node::P2PNode;
pub use messages::{
    FileChunk, FileMeta, FileRequest, FileResponse, RsBlock, RsHave, ZoneRequest, ZoneResponse,
    FILE_CHUNK_SIZE,
};
pub use protocol::PeerEntry;
pub use service::P2PService;
