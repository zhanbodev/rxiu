//! Terminal control utilities.
//!
//! Handles raw mode, screen clearing, and cleanup on exit.

use std::io::{self, Write};

use crossterm::{
    ExecutableCommand, cursor,
    terminal::{self, ClearType},
};

use crate::error::Result;

/// Enter raw mode for interactive UI.
pub fn enter_raw_mode() -> Result<()> {
    terminal::enable_raw_mode()?;
    Ok(())
}

/// Exit raw mode and restore terminal state.
pub fn exit_raw_mode() -> Result<()> {
    terminal::disable_raw_mode()?;
    Ok(())
}

/// Clear the screen and move cursor to top-left.
pub fn clear_screen() -> Result<()> {
    let mut stdout = io::stdout();
    stdout.execute(terminal::Clear(ClearType::All))?;
    stdout.execute(cursor::MoveTo(0, 0))?;
    stdout.flush()?;
    Ok(())
}

/// RAII guard for raw mode - ensures cleanup even on panic.
pub struct RawModeGuard;

impl RawModeGuard {
    pub fn new() -> Result<Self> {
        enter_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = exit_raw_mode();
    }
}
