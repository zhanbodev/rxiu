//! Daemon module for background P2P service.
//!
//! The daemon runs as a separate process, providing P2P connectivity
//! that persists even when the TUI is closed.

pub mod block_server;
pub mod client;
pub mod protocol;
pub mod proxy;
pub mod renew;
pub mod rs_sync;
pub mod server;

pub use client::DaemonClient;
pub use protocol::{DAEMON_PORT, DaemonRequest, DaemonResponse};
pub use proxy::P2PProxy;
