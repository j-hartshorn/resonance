//! Configuration management for room.rs
//!
//! This crate handles loading, saving and accessing
//! application configuration.

use core::Error;
use log::{debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};

/// Application settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    // TODO: Implement in Phase 1
    /// User's display name
    pub username: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            username: "Anonymous".to_string(),
        }
    }
}

/// Configuration manager
pub struct ConfigManager {
    // TODO: Implement in Phase 1
    settings: Settings,
}

impl ConfigManager {
    /// Create a new config manager with default settings
    pub fn new() -> Result<Self, Error> {
        // TODO: Implement loading from file in Phase 1
        Ok(Self {
            settings: Settings::default(),
        })
    }

    /// Get the current settings
    pub fn settings(&self) -> &Settings {
        &self.settings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings() {
        let settings = Settings::default();
        assert_eq!(settings.username, "Anonymous");
    }
}
