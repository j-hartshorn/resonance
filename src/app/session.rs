use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::task::JoinHandle;

use crate::network::{
    discover_public_endpoint, generate_connection_link, parse_connection_link, ConnectionManager,
    ConnectionState, Endpoint, Message,
};
use crate::ui::Participant;

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
    /// The original host's ID, used to maintain session if host leaves
    pub original_host_id: String,
    /// Time the session was created
    pub created_at: u64,
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

/// Peer information for session participants
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Peer {
    /// Unique identifier for the peer
    pub id: String,
    /// Display name of the peer
    pub name: String,
    /// Network endpoint information
    pub endpoint: Endpoint,
    /// Public key for secure communication
    pub public_key: [u8; 32],
    /// Position in virtual space (x, y, z)
    pub position: (f32, f32, f32),
    /// Whether this peer is currently the session host
    pub is_host: bool,
    /// Time this peer joined the session (for host election)
    pub joined_at: u64,
}

/// Manages audio communication sessions
pub struct SessionManager {
    current_session: Option<Session>,
    audio_streams: HashMap<String, Arc<Mutex<Vec<f32>>>>,
    // Changed from Option<ConnectionManager> to a HashMap to support multiple peers
    peer_connections: HashMap<String, ConnectionManager>,
    background_tasks: Vec<JoinHandle<()>>,
    host_public_endpoint: Option<Endpoint>,
    // Track all peers in the session
    peers: HashMap<String, Peer>,
    // Current user's ID
    self_id: String,
}

impl SessionManager {
    /// Creates a new session manager
    pub fn new() -> Self {
        Self {
            current_session: None,
            audio_streams: HashMap::new(),
            peer_connections: HashMap::new(),
            background_tasks: Vec::new(),
            host_public_endpoint: None,
            peers: HashMap::new(),
            self_id: uuid::Uuid::new_v4().to_string(),
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

        // Current timestamp for session creation
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Generate shareable link
        let connection_link = generate_connection_link(&endpoint, &session_id, &public_key);

        // Add ourselves as a peer
        let self_peer = Peer {
            id: self.self_id.clone(),
            name: "Me".to_string(),
            endpoint: endpoint.clone(),
            public_key,
            position: (0.0, 0.0, 0.0),
            is_host: true,
            joined_at: timestamp,
        };

        self.peers.insert(self.self_id.clone(), self_peer);

        // Create session
        let current_user = Participant::new("Me").with_position(0.0, 0.0, 0.0);
        let session = Session {
            id: session_id.clone(),
            connection_link: connection_link.clone(),
            participants: vec![current_user],
            is_host: true,
            original_host_id: self.self_id.clone(),
            created_at: timestamp,
        };

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

        // Host endpoint
        let host_endpoint = Endpoint {
            ip: remote_ip,
            port: remote_port,
        };

        // Host ID
        let host_id = format!("host-{}", session_id);

        // Create connection manager for the host
        let connection_manager =
            ConnectionManager::new(remote_ip, remote_port, session_id.clone(), remote_key);

        // Connect to remote peer
        connection_manager
            .connect()
            .await
            .map_err(|e| SessionError::JoinError(format!("Connection failed: {}", e)))?;

        // Current timestamp for joining
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Add the host to peers list
        let host_peer = Peer {
            id: host_id.clone(),
            name: "Host".to_string(),
            endpoint: host_endpoint,
            public_key: remote_key,
            position: (0.0, 0.0, -1.0),
            is_host: true,
            joined_at: timestamp - 1, // Host joined before us
        };
        self.peers.insert(host_id.clone(), host_peer);

        // Add ourselves to the peers list
        let self_peer = Peer {
            id: self.self_id.clone(),
            name: "Me".to_string(),
            endpoint: Endpoint {
                ip: "0.0.0.0".parse().unwrap(),
                port: 0,
            }, // Will be updated after STUN
            public_key: [0; 32], // Will be set properly later
            position: (0.0, 0.0, 0.0),
            is_host: false,
            joined_at: timestamp,
        };
        self.peers.insert(self.self_id.clone(), self_peer);

        // Start message handler
        let audio_streams = self.audio_streams.clone();
        let peers = Arc::new(Mutex::new(self.peers.clone()));
        let session_id_clone = session_id.clone();
        let self_id_clone = self.self_id.clone();

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
                    Message::PeerList { peers: peer_list } => {
                        // Received peer list from host
                        let mut peers_lock = peers.lock().unwrap();

                        // Update our peer list with the received information
                        for peer in peer_list {
                            // Don't update ourselves
                            if peer.id != self_id_clone {
                                peers_lock.insert(peer.id.clone(), peer);
                            }
                        }

                        // TODO: Connect to other peers in the list
                    }
                    Message::NewPeer { peer } => {
                        // A new peer joined the session
                        let mut peers_lock = peers.lock().unwrap();

                        // Add to our peer list
                        if peer.id != self_id_clone {
                            peers_lock.insert(peer.id.clone(), peer);
                        }

                        // TODO: Connect to this new peer
                    }
                    Message::PeerLeft { peer_id } => {
                        // A peer left the session
                        let mut peers_lock = peers.lock().unwrap();

                        // Remove from our peer list
                        peers_lock.remove(&peer_id);

                        // If the host left, elect a new host
                        let mut new_host = false;
                        for peer in peers_lock.values() {
                            if peer.is_host && peer.id == peer_id {
                                new_host = true;
                                break;
                            }
                        }

                        if new_host {
                            // Simple host election: oldest peer becomes host
                            let mut oldest_time = u64::MAX;
                            let mut oldest_id = String::new();

                            for (id, peer) in peers_lock.iter() {
                                if peer.joined_at < oldest_time {
                                    oldest_time = peer.joined_at;
                                    oldest_id = id.clone();
                                }
                            }

                            // If we're the oldest, we become host
                            if oldest_id == self_id_clone {
                                if let Some(peer) = peers_lock.get_mut(&self_id_clone) {
                                    peer.is_host = true;

                                    // TODO: Notify other peers that we're the new host
                                }
                            }
                        }
                    }
                    // Handle other message types as needed
                    _ => {}
                }

                Ok(())
            })
            .await;

        self.background_tasks.push(handler_task);
        self.peer_connections
            .insert(host_id.clone(), connection_manager);

        // Create local session representation with host and current user
        let current_user = Participant::new("Me").with_position(0.0, 0.0, 0.0);
        let host = Participant::new("Host").with_position(0.0, 0.0, -1.0);

        let session = Session {
            id: session_id,
            connection_link: link.to_string(),
            participants: vec![current_user, host],
            is_host: false,
            original_host_id: host_id,
            created_at: timestamp - 1, // Host created before we joined
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
            // Notify all peers that we're leaving
            for (peer_id, connection) in &self.peer_connections {
                // Skip sending if connection is not active
                if connection.is_connected().await {
                    let _ = connection.send_peer_left(&self.self_id).await;
                }
            }

            // Clear audio streams for all participants
            self.audio_streams.clear();

            // Abort all background tasks
            for task in self.background_tasks.drain(..) {
                task.abort();
            }

            // Clear connection managers
            self.peer_connections.clear();

            // Clear peers list
            self.peers.clear();

            // Clear current session
            self.current_session = None;

            // Clear host endpoint
            self.host_public_endpoint = None;

            Ok(())
        } else {
            Err(SessionError::NoActiveSession)
        }
    }

    /// Gets the current session if available
    pub fn current_session(&self) -> Option<Session> {
        self.current_session.clone()
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

    /// Sends audio data to all connected peers
    pub async fn send_audio_data(&self, audio_data: &[f32]) -> Result<(), SessionError> {
        let mut errors = Vec::new();

        // Send to all connected peers
        for (peer_id, connection) in &self.peer_connections {
            // Skip sending to ourselves
            if peer_id == &self.self_id {
                continue;
            }

            // Skip if connection is not active
            if !connection.is_connected().await {
                continue;
            }

            // Send audio data
            if let Err(e) = connection.send_audio_data(audio_data).await {
                errors.push(format!("Failed to send audio to {}: {}", peer_id, e));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(SessionError::NetworkError(errors.join("; ")))
        }
    }

    /// Checks if the session has a valid connection manager
    pub async fn connection_state(&self) -> Option<ConnectionState> {
        for connection in self.peer_connections.values() {
            let state = connection.connection_state().await;
            if state == ConnectionState::Connected {
                return Some(state);
            }
        }
        None
    }

    /// Connects to a peer in the session
    pub async fn connect_to_peer(&mut self, peer_id: &str) -> Result<(), SessionError> {
        // Get peer info
        let peer = match self.peers.get(peer_id) {
            Some(peer) => peer.clone(),
            None => return Err(SessionError::NetworkError("Peer not found".to_string())),
        };

        // Don't connect to ourselves
        if peer.id == self.self_id {
            return Ok(());
        }

        // Skip if already connected
        if self.peer_connections.contains_key(&peer.id) {
            return Ok(());
        }

        let session_id = match &self.current_session {
            Some(session) => session.id.clone(),
            None => return Err(SessionError::NoActiveSession),
        };

        // Create connection manager
        let connection_manager = ConnectionManager::new(
            peer.endpoint.ip,
            peer.endpoint.port,
            session_id,
            peer.public_key,
        );

        // Connect to peer
        connection_manager
            .connect()
            .await
            .map_err(|e| SessionError::NetworkError(format!("Connection failed: {}", e)))?;

        // Setup message handler
        let audio_streams = self.audio_streams.clone();
        let peers = Arc::new(Mutex::new(self.peers.clone()));
        let self_id_clone = self.self_id.clone();
        let peer_name = peer.name.clone();

        let handler_task = connection_manager
            .start_listening(move |message| {
                match message {
                    Message::Audio { data, timestamp: _ } => {
                        // Convert audio data to f32 samples
                        let samples: Vec<f32> = data.iter().map(|&b| (b as f32) / 255.0).collect();

                        // Store audio stream for this peer
                        if let Some(stream) = audio_streams.get(&peer_name) {
                            let mut stream = stream.lock().unwrap();
                            *stream = samples;
                        }
                    }
                    Message::PeerLeft { peer_id } => {
                        // A peer left the session
                        let mut peers_lock = peers.lock().unwrap();
                        peers_lock.remove(&peer_id);

                        // Handle host leaving
                        let mut new_host_needed = false;
                        for peer in peers_lock.values() {
                            if peer.is_host && peer.id == peer_id {
                                new_host_needed = true;
                                break;
                            }
                        }

                        if new_host_needed {
                            // Simple host election: oldest peer becomes host
                            let mut oldest_time = u64::MAX;
                            let mut oldest_id = String::new();

                            for (id, peer) in peers_lock.iter() {
                                if peer.joined_at < oldest_time {
                                    oldest_time = peer.joined_at;
                                    oldest_id = id.clone();
                                }
                            }

                            // If we're the oldest, we become host
                            if oldest_id == self_id_clone {
                                if let Some(peer) = peers_lock.get_mut(&self_id_clone) {
                                    peer.is_host = true;
                                }
                            }
                        }
                    }
                    // Handle other message types as needed
                    _ => {}
                }

                Ok(())
            })
            .await;

        self.background_tasks.push(handler_task);
        self.peer_connections
            .insert(peer.id.clone(), connection_manager);

        // Initialize audio stream for this peer
        self.audio_streams
            .insert(peer.name.clone(), Arc::new(Mutex::new(Vec::new())));

        Ok(())
    }

    /// Synchronizes the list of peers with all connected peers
    pub async fn sync_peers(&mut self) -> Result<(), SessionError> {
        // Only the host should send the peer list
        let is_host = match &self.current_session {
            Some(session) => session.is_host,
            None => return Err(SessionError::NoActiveSession),
        };

        if !is_host {
            return Ok(());
        }

        // Send peer list to all peers
        let peers: Vec<Peer> = self.peers.values().cloned().collect();

        for (peer_id, connection) in &self.peer_connections {
            // Skip sending to ourselves
            if peer_id == &self.self_id {
                continue;
            }

            // Skip sending if connection is not active
            if connection.is_connected().await {
                let _ = connection.send_peer_list(&peers).await;
            }
        }

        Ok(())
    }

    /// Sends a notification that a new peer has joined
    pub async fn notify_new_peer(&mut self, peer: &Peer) -> Result<(), SessionError> {
        // Only the host should send new peer notifications
        let is_host = match &self.current_session {
            Some(session) => session.is_host,
            None => return Err(SessionError::NoActiveSession),
        };

        if !is_host {
            return Ok(());
        }

        // Send notification to all peers
        for (peer_id, connection) in &self.peer_connections {
            // Skip sending to ourselves and to the new peer
            if peer_id == &self.self_id || peer_id == &peer.id {
                continue;
            }

            // Skip sending if connection is not active
            if connection.is_connected().await {
                let _ = connection.send_new_peer(peer).await;
            }
        }

        Ok(())
    }

    /// Checks if the session has an active connection
    pub async fn has_active_connection(&self) -> bool {
        // First check if we have a current session
        if self.current_session.is_none() {
            return false;
        }

        // If we have a session but no peer connections, return true if we're the host
        if self.peer_connections.is_empty() {
            return self.current_session.as_ref().map_or(false, |s| s.is_host);
        }

        // Otherwise check for active peer connections
        for connection in self.peer_connections.values() {
            if connection.is_connected().await {
                return true;
            }
        }
        false
    }
}

impl Clone for SessionManager {
    fn clone(&self) -> Self {
        SessionManager {
            current_session: self.current_session.clone(),
            audio_streams: self.audio_streams.clone(),
            peer_connections: self.peer_connections.clone(),
            background_tasks: Vec::new(), // Don't clone background tasks
            host_public_endpoint: self.host_public_endpoint.clone(),
            peers: self.peers.clone(),
            self_id: self.self_id.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    #[test]
    fn test_session_creation() {
        let session = Session {
            id: "test-id".to_string(),
            connection_link: "test-link".to_string(),
            participants: vec![Participant::new("Me")],
            is_host: true,
            original_host_id: "test-id".to_string(),
            created_at: 0,
        };

        assert_eq!(session.id, "test-id");
        assert_eq!(session.connection_link, "test-link");
        assert_eq!(session.participants.len(), 1);
        assert!(session.is_host);
    }

    #[test]
    fn test_peer_creation() {
        let peer = Peer {
            id: "test-peer".to_string(),
            name: "Test Peer".to_string(),
            endpoint: Endpoint {
                ip: "127.0.0.1".parse().unwrap(),
                port: 8080,
            },
            public_key: [0; 32],
            position: (1.0, 0.0, 1.0),
            is_host: false,
            joined_at: 100,
        };

        assert_eq!(peer.id, "test-peer");
        assert_eq!(peer.name, "Test Peer");
        assert_eq!(peer.endpoint.port, 8080);
        assert!(!peer.is_host);
    }

    #[tokio::test]
    async fn test_session_manager_creation() {
        let manager = SessionManager::new();
        assert!(manager.current_session().is_none());
        assert_eq!(manager.audio_streams.len(), 0);
        assert_eq!(manager.peer_connections.len(), 0);
        assert_eq!(manager.peers.len(), 0);
    }

    #[test]
    fn test_session_error_creation() {
        let error = SessionError::NoActiveSession;
        assert!(format!("{}", error).contains("No active session"));

        let error = SessionError::CreationError("test error".to_string());
        assert!(format!("{}", error).contains("test error"));
    }

    #[test]
    fn test_clone_implementation() {
        let session = Session {
            id: "test-id".to_string(),
            connection_link: "test-link".to_string(),
            participants: vec![Participant::new("Me")],
            is_host: true,
            original_host_id: "test-id".to_string(),
            created_at: 0,
        };

        let cloned = session.clone();
        assert_eq!(session.id, cloned.id);
        assert_eq!(session.participants.len(), cloned.participants.len());
    }

    // More complex tests for peer interactions would be done with integration tests
}
