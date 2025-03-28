use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

/// Session information returned after creating or joining a session
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub session_id: String,
    pub connection_link: String,
}

/// A peer in a signaling session
#[derive(Debug, Clone)]
pub struct Peer {
    pub id: String,
    pub name: String,
}

/// Interface for signaling services
#[async_trait]
pub trait SignalingInterface: Send + Sync {
    /// Connect to the signaling service
    async fn connect(&mut self) -> Result<()>;

    /// Disconnect from the signaling service
    async fn disconnect(&mut self) -> Result<()>;

    /// Create a new session
    async fn create_session(&mut self) -> Result<SessionInfo>;

    /// Join an existing session
    async fn join_session(&mut self, link: &str) -> Result<SessionInfo>;
}

/// A signaling service for connection establishment
pub struct SignalingService {
    connected: bool,
    session_id: Option<String>,
    peers: Arc<Mutex<HashMap<String, Peer>>>,
}

#[async_trait]
impl SignalingInterface for SignalingService {
    async fn connect(&mut self) -> Result<()> {
        // In a real implementation, this would establish a connection to a signaling server
        self.connected = true;
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        // In a real implementation, this would disconnect from the signaling server
        self.connected = false;
        self.session_id = None;
        Ok(())
    }

    async fn create_session(&mut self) -> Result<SessionInfo> {
        if !self.connected {
            return Err(anyhow!("Not connected to signaling service"));
        }

        // Generate a unique session ID
        let session_id = Uuid::new_v4().to_string();

        // Create a link that others can use to join
        let connection_link = format!("resonance://join/{}", session_id);

        // Set as our current session
        self.session_id = Some(session_id.clone());

        Ok(SessionInfo {
            session_id,
            connection_link,
        })
    }

    async fn join_session(&mut self, link: &str) -> Result<SessionInfo> {
        if !self.connected {
            return Err(anyhow!("Not connected to signaling service"));
        }

        // Parse the connection link to extract the session ID
        let session_id = link
            .strip_prefix("resonance://join/")
            .ok_or_else(|| anyhow!("Invalid connection link format"))?;

        if session_id.is_empty() {
            return Err(anyhow!("Invalid session ID"));
        }

        // Set as our current session
        self.session_id = Some(session_id.to_string());

        Ok(SessionInfo {
            session_id: session_id.to_string(),
            connection_link: link.to_string(),
        })
    }
}

impl SignalingService {
    /// Create a new signaling service
    pub fn new() -> Self {
        Self {
            connected: false,
            session_id: None,
            peers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get the current session ID
    pub fn current_session_id(&self) -> Option<String> {
        self.session_id.clone()
    }

    /// Add a peer to the current session
    pub fn add_peer(&mut self, id: &str, name: &str) -> Result<()> {
        if !self.connected {
            return Err(anyhow!("Not connected to signaling service"));
        }

        let mut peers = self.peers.lock().unwrap();
        peers.insert(
            id.to_string(),
            Peer {
                id: id.to_string(),
                name: name.to_string(),
            },
        );

        Ok(())
    }

    /// Get all peers in the current session
    pub fn get_peers(&self) -> Vec<Peer> {
        let peers = self.peers.lock().unwrap();
        peers.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::timeout;

    #[tokio::test]
    async fn test_signaling_connection() {
        let mut signaling = SignalingService::new();
        let result = signaling.connect().await;
        assert!(result.is_ok());
        assert!(signaling.connected);
    }

    #[tokio::test]
    async fn test_session_creation() {
        let mut signaling = SignalingService::new();
        signaling.connect().await.unwrap();

        let session_info = signaling.create_session().await.unwrap();
        assert!(!session_info.session_id.is_empty());
        assert!(!session_info.connection_link.is_empty());
        assert!(session_info
            .connection_link
            .contains(&session_info.session_id));
    }

    #[tokio::test]
    async fn test_join_session() {
        let mut signaling = SignalingService::new();
        signaling.connect().await.unwrap();

        let link = "resonance://join/test-session-id";
        let session_info = signaling.join_session(link).await.unwrap();

        assert_eq!(session_info.session_id, "test-session-id");
        assert_eq!(session_info.connection_link, link);
        assert_eq!(signaling.current_session_id().unwrap(), "test-session-id");
    }

    #[tokio::test]
    async fn test_invalid_connection_link() {
        let mut signaling = SignalingService::new();
        signaling.connect().await.unwrap();

        let result = signaling.join_session("invalid-link").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_disconnect() {
        let mut signaling = SignalingService::new();
        signaling.connect().await.unwrap();

        let session_info = signaling.create_session().await.unwrap();
        assert!(signaling.current_session_id().is_some());

        let result = signaling.disconnect().await;
        assert!(result.is_ok());
        assert!(!signaling.connected);
        assert!(signaling.current_session_id().is_none());
    }
}
