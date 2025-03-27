pub mod config;

use std::fs;
use std::path::Path;
use std::str::FromStr;

use config::Config;

/// Main application struct that coordinates all components
pub struct App {
    initialized: bool,
    config: Config,
}

impl App {
    /// Creates a new application instance with default configuration
    pub fn new() -> Self {
        Self {
            initialized: true,
            config: Config::default(),
        }
    }

    /// Creates a new application instance with a specified configuration
    pub fn with_config(config: Config) -> Self {
        Self {
            initialized: true,
            config,
        }
    }

    /// Returns whether the app is initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Gets a reference to the current configuration
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Updates the application configuration
    pub fn update_config(&mut self, config: Config) {
        self.config = config;
    }

    /// Loads configuration from a file
    pub fn load_config<P: AsRef<Path>>(&mut self, path: P) -> Result<(), String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;
            
        let config = Config::from_str(&content)
            .map_err(|e| format!("Failed to parse config: {}", e))?;
            
        self.config = config;
        Ok(())
    }

    /// Saves configuration to a file
    pub fn save_config<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        let content = self.config.to_string();
        
        fs::write(path, content)
            .map_err(|e| format!("Failed to write config file: {}", e))?;
            
        Ok(())
    }

    /// Shuts down the application, releasing resources
    pub fn shutdown(&mut self) -> Result<(), String> {
        self.initialized = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::{Config, AudioQuality};
    use std::fs;
    use std::io::Write;
    
    #[test]
    fn test_app_creation() {
        let app = App::new();
        assert!(app.is_initialized());
    }
    
    #[test]
    fn test_app_shutdown() {
        let mut app = App::new();
        let result = app.shutdown();
        assert!(result.is_ok());
        assert!(!app.is_initialized());
    }
    
    #[test]
    fn test_app_config() {
        let mut app = App::new();
        assert_eq!(app.config().audio_quality, AudioQuality::Medium);
        
        let mut custom_config = Config::default();
        custom_config.audio_quality = AudioQuality::High;
        custom_config.username = "TestUser".to_string();
        
        app.update_config(custom_config.clone());
        assert_eq!(app.config().audio_quality, AudioQuality::High);
        assert_eq!(app.config().username, "TestUser");
    }
    
    #[test]
    fn test_config_load_save() {
        // Create a temporary file for testing
        let temp_file = "test_config.tmp";
        
        // Create a custom config and app
        let mut custom_config = Config::default();
        custom_config.audio_quality = AudioQuality::High;
        custom_config.username = "TestUser".to_string();
        
        let app = App::with_config(custom_config);
        
        // Save the config
        app.save_config(temp_file).unwrap();
        
        // Create a new app and load the config
        let mut new_app = App::new();
        new_app.load_config(temp_file).unwrap();
        
        // Check that the configs match
        assert_eq!(app.config(), new_app.config());
        
        // Clean up
        let _ = fs::remove_file(temp_file);
    }
}