//! Application-wide error types.
//!
//! Uses `thiserror` for ergonomic error derivation while maintaining
//! explicit control over error messages shown to users.

use std::path::PathBuf;
use thiserror::Error;

/// All error conditions that can occur in the file zone system.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("Zone '{0}' already exists")]
    ZoneAlreadyExists(String),

    #[error("Zone '{0}' not found")]
    ZoneNotFound(String),

    #[error("No active zone. Use 'use <zone_name>' to select one")]
    NoActiveZone,

    #[error("File '{0}' not found in current zone")]
    FileNotFound(String),

    #[error("File '{0}' already exists in zone. Rename or remove it first")]
    FileAlreadyExists(String),

    #[error("Path does not exist: {0}")]
    PathNotFound(PathBuf),

    #[error("Not a file: {0}")]
    NotAFile(PathBuf),

    #[error("Operation cancelled by user")]
    Cancelled,

    #[error("Invalid command. Type 'help' for available commands")]
    InvalidCommand,

    #[error("Missing argument: {0}")]
    MissingArgument(&'static str),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Data serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Could not determine home directory")]
    NoHomeDirectory,
}

pub type Result<T> = std::result::Result<T, AppError>;
