//! Updater - handles version checking and binary updates.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::error::Result;

use super::protocol::RenewResponse;
use super::version::VersionInfo;

/// Updater state and configuration.
pub struct Updater {
    /// Whether auto-update is enabled.
    pub enabled: bool,
    /// Check interval in seconds.
    pub check_interval: u64,
    /// Path to store downloaded updates.
    staging_dir: PathBuf,
}

impl Default for Updater {
    fn default() -> Self {
        Self {
            enabled: true,
            check_interval: 300, // 5 minutes
            staging_dir: dirs::home_dir()
                .unwrap_or_default()
                .join(".rxiu")
                .join("updates"),
        }
    }
}

impl Updater {
    /// Create a new updater.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the staging directory for downloads.
    pub fn staging_dir(&self) -> &PathBuf {
        &self.staging_dir
    }

    /// Ensure staging directory exists.
    pub fn ensure_staging_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.staging_dir)?;
        Ok(())
    }

    /// Get path for staging a new binary.
    pub fn staging_path(&self, hash: &str) -> PathBuf {
        self.staging_dir.join(format!("rxiu-daemon.{}", &hash[..8]))
    }

    /// Verify a downloaded binary against expected hash.
    pub fn verify_binary(&self, path: &PathBuf, expected_hash: &str) -> Result<bool> {
        let mut file = File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1024 * 1024];

        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }

        let actual_hash = format!("{:x}", hasher.finalize());
        Ok(actual_hash == expected_hash)
    }

    /// Apply an update by replacing the current binary.
    /// Returns the path to the backup of the old binary.
    pub fn apply_update(&self, new_binary: &PathBuf) -> Result<PathBuf> {
        let current_exe = std::env::current_exe()?;

        // Create backup
        let backup_path = current_exe.with_extension("bak");
        fs::copy(&current_exe, &backup_path)?;

        // On Unix, we can replace the binary even while running
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            // Copy new binary to temp location next to current
            let temp_path = current_exe.with_extension("new");
            fs::copy(new_binary, &temp_path)?;

            // Set executable permissions
            let mut perms = fs::metadata(&temp_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&temp_path, perms)?;

            // Atomic rename
            fs::rename(&temp_path, &current_exe)?;
        }

        #[cfg(windows)]
        {
            // On Windows, we need to use a different approach
            // The running process holds a handle to the exe
            // We'll write a batch script to replace it on next boot
            return Err(crate::error::AppError::Io(std::io::Error::other(
                "Windows update not yet implemented",
            )));
        }

        Ok(backup_path)
    }

    /// Handle a GetVersion request - return current version info.
    pub fn handle_get_version(&self) -> Result<RenewResponse> {
        match VersionInfo::current() {
            Ok(info) => Ok(RenewResponse::Version(info)),
            Err(e) => Ok(RenewResponse::Error(e.to_string())),
        }
    }

    /// Handle a GetBinaryChunk request - return a chunk of the binary.
    pub fn handle_get_binary_chunk(&self, offset: u64, length: u32) -> Result<RenewResponse> {
        let exe_path = std::env::current_exe()?;
        let mut file = File::open(&exe_path)?;
        let file_size = file.metadata()?.len();

        if offset >= file_size {
            return Ok(RenewResponse::BinaryChunk {
                offset,
                data: Vec::new(),
                is_last: true,
            });
        }

        file.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; length as usize];
        let n = file.read(&mut buf)?;
        buf.truncate(n);

        let is_last = offset + n as u64 >= file_size;

        Ok(RenewResponse::BinaryChunk {
            offset,
            data: buf,
            is_last,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_updater_default() {
        let updater = Updater::new();
        assert!(updater.enabled);
        assert_eq!(updater.check_interval, 300);
    }
}
