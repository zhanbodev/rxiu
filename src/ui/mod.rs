//! Interactive terminal UI components.
//!
//! Provides vim-style navigation and file selection using crossterm
//! for cross-platform terminal control.

pub mod browser;
pub mod terminal;

pub use browser::{BrowserMode, FileBrowser};
