//! Zone manager for handling multiple zones.
//!
//! Tracks all created zones and the currently active zone.
//! Persists zone registry to disk for persistence across sessions.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{LocalBackend, Zone};
use crate::error::{AppError, Result};

/// Persisted zone registry.
#[derive(Debug, Serialize, Deserialize, Default)]
struct ZoneRegistry {
    zones: Vec<String>,
}

/// Manages all zones and tracks the active zone.
pub struct ZoneManager {
    /// Path to zones registry file
    registry_path: PathBuf,
    /// Loaded zones (lazily populated)
    zones: HashMap<String, Zone>,
    /// Name of currently active zone
    active_zone: Option<String>,
}

impl ZoneManager {
    /// Initialize the zone manager, loading registry from disk.
    pub fn new() -> Result<Self> {
        let home = dirs::home_dir().ok_or(AppError::NoHomeDirectory)?;
        let config_dir = home.join(".rxiu");
        fs::create_dir_all(&config_dir)?;

        let registry_path = config_dir.join("zones.json");

        // Load existing registry or create empty one
        let registry = if registry_path.exists() {
            let content = fs::read_to_string(&registry_path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            ZoneRegistry::default()
        };

        // Initialize zones from registry
        let mut zones = HashMap::new();
        for zone_name in registry.zones {
            if let Ok(backend) = LocalBackend::new(&zone_name) {
                let zone = Zone::new(zone_name.clone(), Arc::new(backend));
                zones.insert(zone_name, zone);
            }
        }

        Ok(Self {
            registry_path,
            zones,
            active_zone: None,
        })
    }

    /// Save the current zone registry to disk.
    fn save_registry(&self) -> Result<()> {
        let registry = ZoneRegistry {
            zones: self.zones.keys().cloned().collect(),
        };
        let json = serde_json::to_string_pretty(&registry)?;
        fs::write(&self.registry_path, json)?;
        Ok(())
    }

    /// Create a new zone.
    pub fn create_zone(&mut self, name: &str) -> Result<()> {
        if self.zones.contains_key(name) {
            return Err(AppError::ZoneAlreadyExists(name.to_string()));
        }

        let backend = LocalBackend::new(name)?;
        let zone = Zone::new(name.to_string(), Arc::new(backend));
        self.zones.insert(name.to_string(), zone);
        self.save_registry()?;

        Ok(())
    }

    /// Set the active zone.
    pub fn set_active(&mut self, name: &str) -> Result<()> {
        if !self.zones.contains_key(name) {
            return Err(AppError::ZoneNotFound(name.to_string()));
        }

        self.active_zone = Some(name.to_string());
        Ok(())
    }

    /// Get the active zone.
    pub fn active_zone(&self) -> Result<&Zone> {
        match &self.active_zone {
            Some(name) => self.zones.get(name).ok_or(AppError::NoActiveZone),
            None => Err(AppError::NoActiveZone),
        }
    }

    /// Get the active zone name.
    pub fn active_zone_name(&self) -> Option<&str> {
        self.active_zone.as_deref()
    }

    /// Clear the active zone.
    pub fn clear_active(&mut self) {
        self.active_zone = None;
    }

    /// List all zone names.
    pub fn list_zones(&self) -> Vec<&str> {
        self.zones.keys().map(|s| s.as_str()).collect()
    }

    /// Get a zone by name.
    pub fn get_zone(&self, name: &str) -> Option<&Zone> {
        self.zones.get(name)
    }
}
