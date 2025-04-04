//! Configuration management for room.rs
//!
//! This crate handles loading, saving and accessing
//! application configuration.

use log::{debug, error, info, trace, warn};
use room_core::Error;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Application settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// User's display name
    pub username: String,

    /// Preferred audio input device (empty string means system default)
    pub audio_input_device: String,

    /// Preferred audio output device (empty string means system default)
    pub audio_output_device: String,

    /// List of STUN/TURN servers for WebRTC connectivity
    pub ice_servers: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            username: "Anonymous".to_string(),
            audio_input_device: "".to_string(),
            audio_output_device: "".to_string(),
            ice_servers: vec![
                "stun:stun.l.google.com:19302".to_string(),
                "stun:stun.l.google.com:5349".to_string(),
                "stun:stun1.l.google.com:3478".to_string(),
                "stun:stun1.l.google.com:5349".to_string(),
                "stun:stun2.l.google.com:19302".to_string(),
                "stun:stun2.l.google.com:5349".to_string(),
                "stun:stun3.l.google.com:3478".to_string(),
                "stun:stun3.l.google.com:5349".to_string(),
                "stun:stun4.l.google.com:19302".to_string(),
                "stun:stun4.l.google.com:5349".to_string(),
            ],
        }
    }
}

/// Configuration manager
pub struct ConfigManager {
    settings: Settings,
    config_file: PathBuf,
}

impl ConfigManager {
    /// Create a new config manager with default settings
    pub fn new() -> Result<Self, Error> {
        // Get user's config directory
        let mut config_dir = dirs::config_dir()
            .ok_or_else(|| Error::Config("Failed to determine config directory".to_string()))?;
        config_dir.push("room_rs");

        // Create config directory if it doesn't exist
        if !config_dir.exists() {
            fs::create_dir_all(&config_dir)
                .map_err(|e| Error::Config(format!("Failed to create config directory: {}", e)))?;
        }

        let config_file = config_dir.join("config.toml");

        // Try to load config from file, fall back to defaults if not found
        let settings = if config_file.exists() {
            Self::load_from_file(&config_file)?
        } else {
            debug!("Config file not found, using defaults");
            Settings::default()
        };

        Ok(Self {
            settings,
            config_file,
        })
    }

    /// Create a new ConfigManager with a custom file path (mainly for testing)
    pub fn with_file<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let config_file = path.as_ref().to_path_buf();
        let settings = if config_file.exists() {
            Self::load_from_file(&config_file)?
        } else {
            Settings::default()
        };

        Ok(Self {
            settings,
            config_file,
        })
    }

    /// Load settings from a TOML file
    fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Settings, Error> {
        let contents = fs::read_to_string(path)
            .map_err(|e| Error::Config(format!("Failed to read config file: {}", e)))?;

        toml::from_str(&contents)
            .map_err(|e| Error::Config(format!("Failed to parse config file: {}", e)))
    }

    /// Save settings to the config file
    pub fn save(&self) -> Result<(), Error> {
        let toml = toml::to_string_pretty(&self.settings)
            .map_err(|e| Error::Config(format!("Failed to serialize settings: {}", e)))?;

        // Ensure parent directory exists
        if let Some(parent) = self.config_file.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| {
                    Error::Config(format!("Failed to create config directory: {}", e))
                })?;
            }
        }

        fs::write(&self.config_file, toml)
            .map_err(|e| Error::Config(format!("Failed to write config file: {}", e)))?;

        debug!("Saved config to {:?}", self.config_file);
        Ok(())
    }

    /// Get the current settings
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Get a mutable reference to settings
    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    /// Update settings with a new value
    pub fn update_settings(&mut self, new_settings: Settings) {
        self.settings = new_settings;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn default_settings() {
        let settings = Settings::default();
        assert_eq!(settings.username, "Anonymous");
        assert_eq!(settings.audio_input_device, "");
        assert_eq!(settings.audio_output_device, "");
        assert!(!settings.ice_servers.is_empty());
    }

    #[test]
    fn save_and_load() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("test_config.toml");

        // Create a config manager and modify settings
        let mut config = ConfigManager::with_file(&config_path).unwrap();
        config.settings_mut().username = "TestUser".to_string();
        config.settings_mut().audio_input_device = "TestMic".to_string();

        // Save the settings
        config.save().unwrap();
        assert!(config_path.exists());

        // Load the settings in a new manager
        let loaded_config = ConfigManager::with_file(&config_path).unwrap();
        assert_eq!(loaded_config.settings().username, "TestUser");
        assert_eq!(loaded_config.settings().audio_input_device, "TestMic");
    }

    #[test]
    fn file_not_found_uses_defaults() {
        let temp_dir = tempdir().unwrap();
        let nonexistent_path = temp_dir.path().join("nonexistent.toml");

        // Should not error, but use defaults
        let config = ConfigManager::with_file(&nonexistent_path).unwrap();
        assert_eq!(config.settings().username, "Anonymous");
    }
}
