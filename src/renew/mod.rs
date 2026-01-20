//! P2P Auto-Update (Renew) Module.
//!
//! Provides decentralized version distribution across nodes.
//! When one node has a new version, it automatically distributes
//! to other nodes in the network.

pub mod protocol;
pub mod updater;
pub mod version;

pub use updater::Updater;
pub use version::VersionInfo;
