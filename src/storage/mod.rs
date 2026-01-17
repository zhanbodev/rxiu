//! Storage layer for file zones.
//!
//! This module provides trait-based abstractions that decouple zone logic
//! from the underlying storage mechanism, enabling future backends like
//! IPFS, P2P networks, or encrypted storage.

pub mod backend;
pub mod local;
pub mod manager;
pub mod metadata;
pub mod zone;

pub use backend::StorageBackend;
pub use local::LocalBackend;
pub use manager::ZoneManager;
pub use metadata::FileMetadata;
pub use zone::Zone;
