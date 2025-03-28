use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::task::JoinHandle;

use crate::network::{
    discover_public_endpoint, generate_connection_link, parse_connection_link, ConnectionManager,
    ConnectionState, Endpoint, Message,
};
use crate::ui::{qr_code::display_connection_options, Participant};

/// Represents a communication session
#[derive(Debug, Clone)]
pub struct Session {
    /// Unique identifier for the session
    pub id: String,
    /// Link for others to join this session
    pub connection_link: String,
    /// Participants in the session
    pub participants: Vec<Participant>,
    /// Whether the current user is the host
    pub is_host: bool,
}

/// Error types for session operations
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("Failed to create session: {0}")]
    CreationError(String),

    #[error("Failed to join session: {0}")]
    JoinError(String),

    #[error("Failed to leave session: {0}")]
    LeaveError(String),

    #[error("No active session")]
    NoActiveSession,

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("UI error: {0}")]
    UiError(String),
}

impl From<anyhow::Error> for SessionError {
    fn from(error: anyhow::Error) -> Self {
        SessionError::UiError(error.to_string())
    }
}

/// Manages audio communication sessions
pub struct SessionManager {
    current_session: Option<Session>,
    audio_streams: HashMap<String, Arc<Mutex<Vec<f32>>>>,
    connection_manager: Option<ConnectionManager>,
    background_tasks: Vec<JoinHandle<()>>,
    host_public_endpoint: Option<Endpoint>,
}

impl SessionManager {
    /// Creates a new session manager
    pub fn new() -> Self {
        Self {
            current_session: None,
            audio_streams: HashMap::new(),
            connection_manager: None,
            background_tasks: Vec::new(),
            host_public_endpoint: None,
        }
    }

    /// Creates a new P2P session
    pub async fn create_p2p_session(&mut self) -> Result<Session, SessionError> {
        // First leave any existing session
        if self.current_session.is_some() {
            self.leave_session().await?;
        }

        // Discover public IP and port via STUN
        let endpoint = discover_public_endpoint()
            .await
            .map_err(|e| SessionError::CreationError(format!("IP discovery failed: {}", e)))?;

        // Save host endpoint
        self.host_public_endpoint = Some(endpoint.clone());

        // Generate session ID
        let session_id = uuid::Uuid::new_v4().to_string();

        // Generate keypair
        let keypair = crate::network::Keypair::generate();
        let public_key = keypair.public.to_bytes();

        // Generate shareable link
        let connection_link = generate_connection_link(&endpoint, &session_id, &public_key);

        // Create session
        let current_user = Participant::new("Me").with_position(0.0, 0.0, 0.0);
        let session = Session {
            id: session_id.clone(),
            connection_link: connection_link.clone(),
            participants: vec![current_user],
            is_host: true,
        };

        // Display connection options for sharing
        display_connection_options(&connection_link)?;

        // Start listening for incoming connections
        // TODO: Implement a listener for incoming connections

        self.current_session = Some(session.clone());
        Ok(session)
    }

    /// Joins an existing session using a connection link
    pub async fn join_p2p_session(&mut self, link: &str) -> Result<(), SessionError> {
        // First leave any existing session
        if self.current_session.is_some() {
            self.leave_session().await?;
        }

        // Parse connection link
        let (remote_ip, remote_port, session_id, remote_key) = parse_connection_link(link)
            .map_err(|e| SessionError::JoinError(format!("Invalid link: {}", e)))?;

        // Create connection manager
        let connection_manager =
            ConnectionManager::new(remote_ip, remote_port, session_id.clone(), remote_key);

        // Connect to remote peer
        connection_manager
            .connect()
            .await
            .map_err(|e| SessionError::JoinError(format!("Connection failed: {}", e)))?;

        // Start message handler
        let audio_streams = self.audio_streams.clone();
        let handler_task = connection_manager
            .start_listening(move |message| {
                match message {
                    Message::Audio { data, timestamp: _ } => {
                        // Convert audio data to f32 samples
                        // This is a simplified example - real implementation would properly convert
                        let samples: Vec<f32> = data.iter().map(|&b| (b as f32) / 255.0).collect();

                        // Store audio stream for "Host"
                        if let Some(stream) = audio_streams.get("Host") {
                            let mut stream = stream.lock().unwrap();
                            *stream = samples;
                        }
                    }
                    _ => {} // Handle other message types as needed
                }

                Ok(())
            })
            .await;

        self.background_tasks.push(handler_task);
        self.connection_manager = Some(connection_manager);

        // Create local session representation with host and current user
        let current_user = Participant::new("Me").with_position(0.0, 0.0, 0.0);
        let host = Participant::new("Host").with_position(0.0, 0.0, -1.0);

        let session = Session {
            id: session_id,
            connection_link: link.to_string(),
            participants: vec![current_user, host],
            is_host: false,
        };

        // Initialize audio stream for host
        self.audio_streams
            .insert("Host".to_string(), Arc::new(Mutex::new(Vec::new())));

        self.current_session = Some(session);
        Ok(())
    }

    /// Leaves the current session
    pub async fn leave_session(&mut self) -> Result<(), SessionError> {
        if self.current_session.is_some() {
            // Clear audio streams for all participants
            self.audio_streams.clear();

            // Abort all background tasks
            for task in self.background_tasks.drain(..) {
                task.abort();
            }

            // Clear connection manager
            self.connection_manager = None;

            self.current_session = None;
            Ok(())
        } else {
            Err(SessionError::NoActiveSession)
        }
    }

    /// Returns a reference to the current session, if any
    pub fn current_session(&self) -> Option<&Session> {
        self.current_session.as_ref()
    }

    /// Adds a participant to the current session
    pub fn add_participant(&mut self, participant: Participant) -> Result<(), SessionError> {
        if let Some(session) = &mut self.current_session {
            session.participants.push(participant.clone());

            // Initialize audio stream buffer for this participant
            self.audio_streams
                .insert(participant.name.clone(), Arc::new(Mutex::new(Vec::new())));
            Ok(())
        } else {
            Err(SessionError::NoActiveSession)
        }
    }

    /// Removes a participant from the current session
    pub fn remove_participant(&mut self, name: &str) -> Result<(), SessionError> {
        if let Some(session) = &mut self.current_session {
            session.participants.retain(|p| p.name != name);

            // Remove audio stream for this participant
            self.audio_streams.remove(name);
            Ok(())
        } else {
            Err(SessionError::NoActiveSession)
        }
    }

    /// Gets the audio stream for a specific participant
    pub fn get_audio_stream(&self, name: &str) -> Option<Arc<Mutex<Vec<f32>>>> {
        self.audio_streams.get(name).cloned()
    }

    /// Updates the audio stream for a specific participant
    pub fn update_audio_stream(
        &mut self,
        name: &str,
        audio_data: Vec<f32>,
    ) -> Result<(), SessionError> {
        if let Some(stream) = self.audio_streams.get(name) {
            if let Ok(mut stream) = stream.lock() {
                *stream = audio_data;
                Ok(())
            } else {
                Err(SessionError::NetworkError(
                    "Failed to lock audio stream".to_string(),
                ))
            }
        } else {
            // If stream doesn't exist yet, create it
            let stream = Arc::new(Mutex::new(audio_data));
            self.audio_streams.insert(name.to_string(), stream);
            Ok(())
        }
    }

    /// Sends audio data to remote peers
    pub async fn send_audio_data(&self, audio_data: &[f32]) -> Result<(), SessionError> {
        if let Some(connection_manager) = &self.connection_manager {
            // Convert f32 samples to bytes for transmission
            // This is a simplified example - real implementation would properly convert
            let bytes: Vec<u8> = audio_data
                .iter()
                .map(|&sample| ((sample * 255.0) as u8))
                .collect();

            // Get current timestamp
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;

            // Send audio data
            connection_manager
                .send_audio(&bytes, timestamp)
                .await
                .map_err(|e| SessionError::NetworkError(e.to_string()))?;

            Ok(())
        } else {
            Err(SessionError::NetworkError(
                "No active connection".to_string(),
            ))
        }
    }

    /// Gets the connection state if available
    pub async fn connection_state(&self) -> Option<ConnectionState> {
        match self.connection_manager.as_ref() {
            Some(cm) => Some(cm.connection_state().await),
            None => None,
        }
    }

    /// Checks if there's an active connection to send audio over
    pub async fn has_active_connection(&self) -> bool {
        match self.connection_manager.as_ref() {
            Some(cm) => {
                // Check if we have a connection manager and it's in Connected state
                match cm.connection_state().await {
                    ConnectionState::Connected => true,
                    _ => false,
                }
            }
            None => false,
        }
    }
}

impl Clone for SessionManager {
    fn clone(&self) -> Self {
        Self {
            current_session: self.current_session.clone(),
            audio_streams: self.audio_streams.clone(),
            connection_manager: None, // Cannot clone ConnectionManager, so create a new one if needed
            background_tasks: Vec::new(), // Don't clone background tasks
            host_public_endpoint: self.host_public_endpoint.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_creation() {
        let session = Session {
            id: "test-id".to_string(),
            connection_link: "test-link".to_string(),
            participants: vec![Participant::new("Me")],
            is_host: true,
        };

        assert_eq!(session.id, "test-id");
        assert_eq!(session.connection_link, "test-link");
        assert_eq!(session.participants.len(), 1);
        assert!(session.is_host);
    }

    #[test]
    fn test_participant_management() {
        let mut session_manager = SessionManager::new();

        // Manually create a session
        let current_user = Participant::new("Me").with_position(0.0, 0.0, 0.0);
        let session = Session {
            id: "test-id".to_string(),
            connection_link: "test-link".to_string(),
            participants: vec![current_user],
            is_host: true,
        };

        session_manager.current_session = Some(session);

        // Add a participant
        let new_participant = Participant::new("User1").with_position(1.0, 0.0, 0.0);
        session_manager
            .add_participant(new_participant.clone())
            .unwrap();

        // Check that participant was added
        let session = session_manager.current_session().unwrap();
        assert_eq!(session.participants.len(), 2);
        assert_eq!(session.participants[1].name, "User1");

        // Remove the participant
        session_manager.remove_participant("User1").unwrap();

        // Check that participant was removed
        let session = session_manager.current_session().unwrap();
        assert_eq!(session.participants.len(), 1);
    }
}
