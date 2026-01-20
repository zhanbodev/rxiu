//! rxiu - Terminal-based file zone management system
//!
//! A decentralization-ready file management tool with isolated zones,
//! full-screen TUI, and P2P LAN synchronization via libp2p.

pub mod cli;
pub mod config;
pub mod daemon;
pub mod error;
pub mod p2p;
pub mod renew;
pub mod rs;
pub mod storage;
pub mod tui;
pub mod ui;

pub use error::{AppError, Result};
