// Configuration management module
// Handles application configuration and settings

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub spatial_enabled: bool,
    pub echo_cancellation: bool,
    pub noise_suppression: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub ice_servers: Vec<String>,
    pub connection_timeout_ms: u64,
    pub max_participants: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub show_spectrograms: bool,
    pub theme: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub audio: AudioConfig,
    pub network: NetworkConfig,
    pub ui: UiConfig,
    pub config_path: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            audio: AudioConfig {
                input_device: None,
                output_device: None,
                spatial_enabled: true,
                echo_cancellation: true,
                noise_suppression: true,
            },
            network: NetworkConfig {
                ice_servers: vec![
                    "stun:stun.l.google.com:19302".to_string(),
                    "stun:stun1.l.google.com:19302".to_string(),
                ],
                connection_timeout_ms: 30000,
                max_participants: 10,
            },
            ui: UiConfig {
                show_spectrograms: true,
                theme: "default".to_string(),
            },
            config_path: None,
        }
    }
}

impl Config {
    pub fn load(path: &PathBuf) -> anyhow::Result<Self> {
        if path.exists() {
            let file = std::fs::File::open(path)?;
            let mut config: Config = serde_json::from_reader(file)?;
            config.config_path = Some(path.clone());
            Ok(config)
        } else {
            let config = Config::default();
            Ok(config)
        }
    }
    
    pub fn save(&self) -> anyhow::Result<()> {
        if let Some(path) = &self.config_path {
            let file = std::fs::File::create(path)?;
            serde_json::to_writer_pretty(file, self)?;
        }
        Ok(())
    }
}