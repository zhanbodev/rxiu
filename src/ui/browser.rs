//! Interactive file browser with vim-style navigation.
//!
//! Provides terminal-based file selection for both `get` (choose destination)
//! and `put` (choose source file) commands.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crossterm::{
    ExecutableCommand, QueueableCommand, cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    style::{self, Stylize},
    terminal::{self, ClearType},
};

use super::terminal::RawModeGuard;
use crate::error::{AppError, Result};

/// Mode determines what the browser is selecting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BrowserMode {
    /// Selecting a file to import (put command)
    SelectFile,
    /// Selecting a directory to save to (get command)
    SelectDirectory,
}

/// Entry in the file browser listing.
#[derive(Debug, Clone)]
struct Entry {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

/// Interactive file browser.
#[derive(Debug)]
pub struct FileBrowser {
    mode: BrowserMode,
    current_dir: PathBuf,
    entries: Vec<Entry>,
    selected: usize,
    scroll_offset: usize,
}

impl FileBrowser {
    /// Create a new file browser starting at the given directory.
    pub fn new(start_dir: &Path, mode: BrowserMode) -> Result<Self> {
        let mut browser = Self {
            mode,
            current_dir: start_dir.to_path_buf(),
            entries: Vec::new(),
            selected: 0,
            scroll_offset: 0,
        };
        browser.refresh_entries()?;
        Ok(browser)
    }

    /// Refresh the entry list from current directory.
    fn refresh_entries(&mut self) -> Result<()> {
        self.entries.clear();
        self.selected = 0;
        self.scroll_offset = 0;

        // Add parent directory entry if not at root
        if let Some(parent) = self.current_dir.parent() {
            self.entries.push(Entry {
                name: "..".to_string(),
                path: parent.to_path_buf(),
                is_dir: true,
            });
        }

        // Read directory contents
        let read_dir = fs::read_dir(&self.current_dir)?;
        let mut dirs = Vec::new();
        let mut files = Vec::new();

        for entry in read_dir.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden files
            if name.starts_with('.') {
                continue;
            }

            let is_dir = path.is_dir();
            let entry = Entry { name, path, is_dir };

            if is_dir {
                dirs.push(entry);
            } else {
                files.push(entry);
            }
        }

        // Sort: directories first, then files, both alphabetically
        dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        self.entries.extend(dirs);
        self.entries.extend(files);

        Ok(())
    }

    /// Run the browser and return the selected path.
    pub fn run(&mut self) -> Result<PathBuf> {
        let _guard = RawModeGuard::new()?;

        loop {
            self.render()?;

            if let Event::Key(key) = event::read()? {
                match self.handle_key(key)? {
                    BrowserAction::Continue => continue,
                    BrowserAction::Select(path) => {
                        // Clear screen before returning
                        let mut stdout = io::stdout();
                        stdout.execute(terminal::Clear(ClearType::All))?;
                        stdout.execute(cursor::MoveTo(0, 0))?;
                        return Ok(path);
                    }
                    BrowserAction::Cancel => {
                        let mut stdout = io::stdout();
                        stdout.execute(terminal::Clear(ClearType::All))?;
                        stdout.execute(cursor::MoveTo(0, 0))?;
                        return Err(AppError::Cancelled);
                    }
                }
            }
        }
    }

    /// Handle a key press.
    fn handle_key(&mut self, key: KeyEvent) -> Result<BrowserAction> {
        match key.code {
            // Quit / Cancel
            KeyCode::Esc | KeyCode::Char('q') => Ok(BrowserAction::Cancel),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Ok(BrowserAction::Cancel)
            }

            // Navigation: up
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.adjust_scroll();
                }
                Ok(BrowserAction::Continue)
            }

            // Navigation: down
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected < self.entries.len().saturating_sub(1) {
                    self.selected += 1;
                    self.adjust_scroll();
                }
                Ok(BrowserAction::Continue)
            }

            // Enter directory / go back
            KeyCode::Enter | KeyCode::Char('l') => {
                if let Some(entry) = self.entries.get(self.selected) {
                    if entry.is_dir {
                        let old_dir = self.current_dir.clone();
                        self.current_dir = entry.path.clone();
                        // Try to refresh - if fails, revert to old directory
                        if let Err(_e) = self.refresh_entries() {
                            self.current_dir = old_dir;
                            let _ = self.refresh_entries(); // Restore old entries
                            // Return Continue, not error - just can't access that dir
                        }
                    } else if self.mode == BrowserMode::SelectFile {
                        // If selecting file and user pressed enter on a file, select it
                        return Ok(BrowserAction::Select(entry.path.clone()));
                    }
                }
                Ok(BrowserAction::Continue)
            }

            // Go to parent
            KeyCode::Backspace | KeyCode::Char('h') => {
                if let Some(parent) = self.current_dir.parent() {
                    let old_dir = self.current_dir.clone();
                    self.current_dir = parent.to_path_buf();
                    if let Err(_e) = self.refresh_entries() {
                        self.current_dir = old_dir;
                        let _ = self.refresh_entries();
                    }
                }
                Ok(BrowserAction::Continue)
            }

            // Confirm selection
            KeyCode::Char('y') => {
                if let Some(entry) = self.entries.get(self.selected) {
                    match self.mode {
                        BrowserMode::SelectFile => {
                            if !entry.is_dir {
                                return Ok(BrowserAction::Select(entry.path.clone()));
                            }
                            // Can't select a directory in file mode
                        }
                        BrowserMode::SelectDirectory => {
                            if entry.is_dir && entry.name != ".." {
                                return Ok(BrowserAction::Select(entry.path.clone()));
                            } else if entry.name == ".." {
                                // Select current directory instead of parent
                                return Ok(BrowserAction::Select(self.current_dir.clone()));
                            }
                        }
                    }
                }
                // If current dir is selected in directory mode (e.g., empty directory)
                if self.mode == BrowserMode::SelectDirectory {
                    return Ok(BrowserAction::Select(self.current_dir.clone()));
                }
                Ok(BrowserAction::Continue)
            }

            _ => Ok(BrowserAction::Continue),
        }
    }

    /// Adjust scroll offset to keep selection visible.
    fn adjust_scroll(&mut self) {
        let (_, height) = terminal::size().unwrap_or((80, 24));
        let visible_rows = (height as usize).saturating_sub(5); // Header + footer

        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + visible_rows {
            self.scroll_offset = self.selected - visible_rows + 1;
        }
    }

    /// Render the browser to the terminal.
    fn render(&self) -> Result<()> {
        let mut stdout = io::stdout();
        let (width, height) = terminal::size().unwrap_or((80, 24));

        stdout.execute(terminal::Clear(ClearType::All))?;
        stdout.execute(cursor::MoveTo(0, 0))?;

        // Header
        let mode_str = match self.mode {
            BrowserMode::SelectFile => "SELECT FILE",
            BrowserMode::SelectDirectory => "SELECT DIRECTORY",
        };
        stdout.queue(style::PrintStyledContent(
            format!(" {} ", mode_str).black().on_cyan(),
        ))?;
        stdout.queue(style::Print("\r\n"))?;

        // Current path
        stdout.queue(style::PrintStyledContent(" 📂 ".dark_yellow()))?;
        stdout.queue(style::PrintStyledContent(
            self.current_dir.display().to_string().cyan(),
        ))?;
        stdout.queue(style::Print("\r\n"))?;

        // Separator
        stdout.queue(style::Print(format!("{}\r\n", "─".repeat(width as usize))))?;

        // Calculate visible area
        let visible_rows = (height as usize).saturating_sub(6);
        let start = self.scroll_offset;
        let end = (start + visible_rows).min(self.entries.len());

        // Entries
        for (i, entry) in self
            .entries
            .iter()
            .enumerate()
            .skip(start)
            .take(end - start)
        {
            let is_selected = i == self.selected;

            // Selection indicator
            if is_selected {
                stdout.queue(style::PrintStyledContent(" ▶ ".green()))?;
            } else {
                stdout.queue(style::Print("   "))?;
            }

            // Icon
            let icon = if entry.is_dir { "📁" } else { "📄" };
            stdout.queue(style::Print(format!("{} ", icon)))?;

            // Name
            let name = if entry.is_dir {
                format!("{}/", entry.name).blue()
            } else {
                entry.name.clone().white()
            };

            if is_selected {
                stdout.queue(style::PrintStyledContent(name.bold()))?;
            } else {
                stdout.queue(style::PrintStyledContent(name))?;
            }

            stdout.queue(style::Print("\r\n"))?;
        }

        // Move to bottom for help
        stdout.execute(cursor::MoveTo(0, height - 2))?;
        stdout.queue(style::Print(format!("{}\r\n", "─".repeat(width as usize))))?;

        // Help line
        stdout.queue(style::PrintStyledContent(" j/k".yellow()))?;
        stdout.queue(style::Print(" navigate  "))?;
        stdout.queue(style::PrintStyledContent("Enter/l".yellow()))?;
        stdout.queue(style::Print(" open  "))?;
        stdout.queue(style::PrintStyledContent("h/Backspace".yellow()))?;
        stdout.queue(style::Print(" back  "))?;
        stdout.queue(style::PrintStyledContent("y".yellow()))?;
        stdout.queue(style::Print(" confirm  "))?;
        stdout.queue(style::PrintStyledContent("q/Esc".yellow()))?;
        stdout.queue(style::Print(" cancel"))?;

        stdout.flush()?;
        Ok(())
    }

    /// Get browser state for external rendering (TUI mode).
    pub fn get_state(&self) -> (&PathBuf, Vec<BrowserEntry>, usize) {
        let entries: Vec<BrowserEntry> = self
            .entries
            .iter()
            .map(|e| BrowserEntry {
                name: e.name.clone(),
                is_dir: e.is_dir,
            })
            .collect();
        (&self.current_dir, entries, self.selected)
    }

    /// Handle key input for TUI mode (doesn't manage raw mode itself).
    /// Returns Ok(Some(path)) on selection, Ok(None) to continue, Err on cancel.
    pub fn handle_key_tui(&mut self, key: KeyEvent) -> Result<Option<PathBuf>> {
        match self.handle_key(key)? {
            BrowserAction::Continue => Ok(None),
            BrowserAction::Select(path) => Ok(Some(path)),
            BrowserAction::Cancel => Err(AppError::Cancelled),
        }
    }
}

/// Public entry info for TUI rendering.
#[derive(Debug, Clone)]
pub struct BrowserEntry {
    pub name: String,
    pub is_dir: bool,
}

/// Result of handling a key press.
enum BrowserAction {
    Continue,
    Select(PathBuf),
    Cancel,
}
