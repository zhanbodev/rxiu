//! Version management for auto-update.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Current version from Cargo.toml at compile time.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Build version based on packaging time.
pub const BUILD_VERSION: &str = env!("RXIU_BUILD_VERSION");

/// Version information for a node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionInfo {
    /// Semantic version string (e.g., "0.3.1")
    pub version: String,
    /// SHA256 hash of the binary
    pub hash: String,
    /// Binary size in bytes
    pub size: u64,
    /// Target OS (e.g., "macos", "windows", "linux")
    pub target_os: String,
    /// Target architecture (e.g., "x86_64", "aarch64")
    pub target_arch: String,
}

impl VersionInfo {
    /// Create version info for the current running binary.
    pub fn current() -> crate::Result<Self> {
        let exe_path = std::env::current_exe()?;
        let (hash, size) = hash_file(&exe_path)?;
        Ok(Self {
            version: BUILD_VERSION.to_string(),
            hash,
            size,
            target_os: std::env::consts::OS.to_string(),
            target_arch: std::env::consts::ARCH.to_string(),
        })
    }

    /// Check if this version is compatible (same platform).
    pub fn is_compatible_with(&self, other: &Self) -> bool {
        self.target_os == other.target_os && self.target_arch == other.target_arch
    }

    /// Check if this version is newer than another.
    pub fn is_newer_than(&self, other: &Self) -> bool {
        // First compare by version string
        if let (Ok(self_ver), Ok(other_ver)) = (
            semver::Version::parse(&self.version),
            semver::Version::parse(&other.version),
        ) {
            if self_ver > other_ver {
                return true;
            }
            if self_ver < other_ver {
                return false;
            }
        }
        // Same version - compare hashes (different build)
        self.hash != other.hash
    }
}

/// Calculate SHA256 hash and size of a file.
pub fn hash_file(path: &Path) -> crate::Result<(String, u64)> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024]; // 1MB buffer
    let mut total_size = 0u64;

    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total_size += n as u64;
    }

    let hash = format!("{:x}", hasher.finalize());
    Ok((hash, total_size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_comparison() {
        let v1 = VersionInfo {
            version: "0.3.0".to_string(),
            hash: "abc".to_string(),
            size: 100,
            target_os: "macos".to_string(),
            target_arch: "x86_64".to_string(),
        };
        let v2 = VersionInfo {
            version: "0.3.1".to_string(),
            hash: "def".to_string(),
            size: 100,
            target_os: "macos".to_string(),
            target_arch: "x86_64".to_string(),
        };
        assert!(v2.is_newer_than(&v1));
        assert!(!v1.is_newer_than(&v2));
    }

    #[test]
    fn test_platform_compatibility() {
        let mac = VersionInfo {
            version: "0.3.0".to_string(),
            hash: "abc".to_string(),
            size: 100,
            target_os: "macos".to_string(),
            target_arch: "x86_64".to_string(),
        };
        let win = VersionInfo {
            version: "0.3.0".to_string(),
            hash: "abc".to_string(),
            size: 100,
            target_os: "windows".to_string(),
            target_arch: "x86_64".to_string(),
        };
        assert!(mac.is_compatible_with(&mac));
        assert!(!mac.is_compatible_with(&win));
    }
}
