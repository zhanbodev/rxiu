//! TUI (Text-based User Interface) module.
//!
//! Provides full-screen terminal interface using ratatui.

pub mod app;
pub mod block_client;
pub mod input;
pub mod render;

pub use app::App;
