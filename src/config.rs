//! Persistent application configuration.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};

const CONFIG_FILE_NAME: &str = "config.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub rs_concurrency: usize,
    pub rs_sync_concurrency: usize,
    pub rs_block_size_mb: u64,
    pub rs_global_sync: bool,
    pub rs_replication_factor: usize,
    /// Enable P2P auto-update
    pub renew_enabled: bool,
    /// Auto-update check interval in seconds
    pub renew_check_interval: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            rs_concurrency: 8,
            rs_sync_concurrency: 8,
            rs_block_size_mb: 16,
            rs_global_sync: true,
            rs_replication_factor: 2,
            renew_enabled: true,
            renew_check_interval: 300, // 5 minutes
        }
    }
}

impl AppConfig {
    pub fn path() -> Result<PathBuf> {
        let home = dirs::home_dir().ok_or(AppError::NoHomeDirectory)?;
        let config_dir = home.join(".rxiu").join("config");
        Ok(config_dir.join(CONFIG_FILE_NAME))
    }

    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            let config = Self::default();
            config.save()?;
            return Ok(config);
        }
        let content = fs::read_to_string(&path)?;
        toml::from_str(&content)
            .map_err(|e| AppError::Io(std::io::Error::other(format!("Invalid config: {}", e))))
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self).map_err(|e| {
            AppError::Io(std::io::Error::other(format!(
                "Config serialize failed: {}",
                e
            )))
        })?;
        fs::write(path, content)?;
        Ok(())
    }
}
