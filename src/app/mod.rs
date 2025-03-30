pub mod config;
pub mod session;
pub mod test_session;

use std::fs;
use std::path::Path;
use std::str::FromStr;

use crate::network::ConnectionState;
use config::Config;
use session::{Session, SessionError, SessionManager};
use test_session::TestSessionManager;

/// Main application struct that coordinates all components
pub struct App {
    initialized: bool,
    config: Config,
    pub session_manager: Option<SessionManager>,
    current_session: Option<Session>,
    test_session_manager: Option<TestSessionManager>,
}

impl App {
    /// Creates a new application instance with default configuration
    pub fn new() -> Self {
        Self {
            initialized: true,
            config: Config::default(),
            session_manager: None,
            current_session: None,
            test_session_manager: None,
        }
    }

    /// Creates a new application instance with a specified configuration
    pub fn with_config(config: Config) -> Self {
        Self {
            initialized: true,
            config,
            session_manager: None,
            current_session: None,
            test_session_manager: None,
        }
    }

    /// Initializes the application
    pub async fn initialize(&mut self) -> Result<(), String> {
        if self.session_manager.is_none() {
            let session_manager = SessionManager::new();
            self.session_manager = Some(session_manager);
        }

        if self.test_session_manager.is_none() {
            let test_session_manager = TestSessionManager::new();
            self.test_session_manager = Some(test_session_manager);
        }

        Ok(())
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
        let content =
            fs::read_to_string(path).map_err(|e| format!("Failed to read config file: {}", e))?;

        let config =
            Config::from_str(&content).map_err(|e| format!("Failed to parse config: {}", e))?;

        self.config = config;
        Ok(())
    }

    /// Saves configuration to a file
    pub fn save_config<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        let content = self.config.to_string();

        fs::write(path, content).map_err(|e| format!("Failed to write config file: {}", e))?;

        Ok(())
    }

    /// Creates a new P2P audio session
    pub async fn create_p2p_session(&mut self) -> Result<Session, String> {
        let session_manager = self
            .session_manager
            .as_mut()
            .ok_or_else(|| "Session manager not initialized".to_string())?;

        session_manager
            .create_p2p_session()
            .await
            .map_err(|e| format!("Failed to create P2P session: {}", e))
    }

    /// Joins an existing P2P session using a connection link
    pub async fn join_p2p_session(&mut self, link: &str) -> Result<(), String> {
        let session_manager = self
            .session_manager
            .as_mut()
            .ok_or_else(|| "Session manager not initialized".to_string())?;

        session_manager
            .join_p2p_session(link)
            .await
            .map_err(|e| format!("Failed to join P2P session: {}", e))
    }

    /// Leaves the current session (regular or test)
    pub async fn leave_session(&mut self) -> Result<(), String> {
        // Check if we have a test session
        if let Some(test_session_manager) = &mut self.test_session_manager {
            if test_session_manager.current_session().is_some() {
                return self.leave_test_session().await;
            }
        }

        // Otherwise try to leave a regular session
        let session_manager = self
            .session_manager
            .as_mut()
            .ok_or_else(|| "Session manager not initialized".to_string())?;

        session_manager.leave_session().await.map_err(|e| match e {
            SessionError::NoActiveSession => "No active session".to_string(),
            _ => format!("Failed to leave session: {}", e),
        })
    }

    /// Gets the current session, if any
    pub fn current_session(&self) -> Option<Session> {
        // First check our cached copy
        if let Some(session) = &self.current_session {
            return Some(session.clone());
        }

        // Then check the test session manager
        if let Some(test_manager) = &self.test_session_manager {
            if let Some(session) = test_manager.current_session() {
                return Some(session);
            }
        }

        // Finally check the regular session manager
        if let Some(sm) = self.session_manager.as_ref() {
            sm.current_session()
        } else {
            None
        }
    }

    /// Gets the current connection state, if any
    pub async fn connection_state(&self) -> Option<ConnectionState> {
        if let Some(sm) = self.session_manager.as_ref() {
            sm.connection_state().await
        } else {
            None
        }
    }

    /// Check if the app has an active connection
    pub async fn has_active_connection(&self) -> bool {
        // Check if we have a regular session
        if let Some(session_manager) = &self.session_manager {
            if session_manager.has_active_connection().await {
                return true;
            }
        }

        // Check if we have a test session
        if let Some(test_session_manager) = &self.test_session_manager {
            if test_session_manager.current_session().is_some() {
                return true;
            }
        }

        false
    }

    /// Arrange participants in a circle for spatial audio
    pub fn arrange_participants_in_circle(&self) -> Vec<(String, (f32, f32, f32))> {
        if let Some(session) = self.current_session() {
            let participants = &session.participants;
            let count = participants.len();

            if count <= 1 {
                return vec![(participants[0].name.clone(), (0.0, 0.0, 0.0))];
            }

            // Use SpatialAudioProcessor to arrange participants
            let mut spatial_processor = crate::audio::SpatialAudioProcessor::new();

            // Find our index in the participants list
            let mut my_index = 0;
            for (i, p) in participants.iter().enumerate() {
                if p.name == "Me" {
                    my_index = i;
                    break;
                }
            }

            // Arrange participants in a circle
            let positions = spatial_processor.arrange_participants_in_circle(count, my_index);

            // Map participant names to positions
            participants
                .iter()
                .enumerate()
                .map(|(i, p)| (p.name.clone(), positions[i]))
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Update participant positions based on spatial arrangement
    pub async fn update_participant_positions(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(positions) = self.arrange_participants_in_circle().into_iter().next() {
            if let Some(session_manager) = &mut self.session_manager {
                // Update session manager with positions
                // This would be implemented to update the peer positions
                // in the actual implementation
            }
        }
        Ok(())
    }

    /// Shuts down the application, releasing resources
    pub async fn shutdown(&mut self) -> Result<(), String> {
        if let Some(session_manager) = self.session_manager.as_mut() {
            if session_manager.current_session().is_some() {
                session_manager
                    .leave_session()
                    .await
                    .map_err(|e| format!("Failed to leave session: {}", e))?;
            }

            self.session_manager = None;
        }

        self.initialized = false;
        Ok(())
    }

    /// Creates a test session with simulated participants
    pub async fn create_test_session(&mut self) -> Result<Session, String> {
        // First leave any existing session
        if self.has_active_connection().await {
            self.leave_session().await?;
        }

        let test_session_manager = self
            .test_session_manager
            .as_mut()
            .ok_or_else(|| "Test session manager not initialized".to_string())?;

        let session = test_session_manager
            .create_test_session()
            .await
            .map_err(|e| format!("Failed to create test session: {}", e))?;

        // Update our current session
        self.current_session = Some(session.clone());

        Ok(session)
    }

    /// Leaves the current test session
    pub async fn leave_test_session(&mut self) -> Result<(), String> {
        let test_session_manager = self
            .test_session_manager
            .as_mut()
            .ok_or_else(|| "Test session manager not initialized".to_string())?;

        test_session_manager
            .leave_test_session()
            .await
            .map_err(|e| format!("Failed to leave test session: {}", e))?;

        // Clear our current session
        self.current_session = None;

        Ok(())
    }

    /// Gets test audio data for a participant
    pub fn get_test_participant_audio(&self, index: usize) -> Vec<f32> {
        if let Some(test_session_manager) = &self.test_session_manager {
            test_session_manager.get_participant_audio(index)
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::{AudioQuality, Config};
    use std::fs;

    #[test]
    fn test_app_creation() {
        let app = App::new();
        assert!(app.is_initialized());
    }

    #[test]
    fn test_app_shutdown() {
        let mut app = App::new();
        tokio_test::block_on(async {
            let result = app.shutdown().await;
            assert!(result.is_ok());
        });
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
        assert!(app.save_config(temp_file).is_ok());

        // Create a new app and load the config
        let mut new_app = App::new();
        assert!(new_app.load_config(temp_file).is_ok());

        // Verify the loaded config matches
        assert_eq!(new_app.config().audio_quality, AudioQuality::High);
        assert_eq!(new_app.config().username, "TestUser");

        // Clean up
        fs::remove_file(temp_file).unwrap();
    }
}
