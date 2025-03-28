use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Information about a session that can be shared with others
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub session_id: String,
    pub connection_link: String,
}

/// A peer in the signaling system
#[derive(Debug, Clone)]
pub struct Peer {
    pub id: String,
    pub name: String,
}

/// The SignalingService handles connection establishment between peers
pub struct SignalingService {
    connected: bool,
    session_id: Option<String>,
    peers: Arc<Mutex<HashMap<String, Peer>>>,
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

    /// Connect to the signaling service
    pub async fn connect(&mut self) -> Result<()> {
        // In a real implementation, this would establish a connection
        // to a signaling server or set up a local peer discovery mechanism

        // For the mock implementation, we'll just set connected to true
        self.connected = true;
        Ok(())
    }

    /// Disconnect from the signaling service
    pub async fn disconnect(&mut self) -> Result<()> {
        if !self.connected {
            return Err(anyhow!("Not connected to signaling service"));
        }

        self.connected = false;
        self.session_id = None;
        Ok(())
    }

    /// Create a new session for communication
    pub async fn create_session(&mut self) -> Result<SessionInfo> {
        if !self.connected {
            return Err(anyhow!("Not connected to signaling service"));
        }

        // Generate a unique session ID
        let session_id = Uuid::new_v4().to_string();

        // Generate a shareable connection link
        let connection_link = format!("resonance://{}", session_id);

        // Store the session ID
        self.session_id = Some(session_id.clone());

        // Return the session info
        Ok(SessionInfo {
            session_id,
            connection_link,
        })
    }

    /// Join an existing session using a connection link
    pub async fn join_session(&mut self, connection_link: &str) -> Result<SessionInfo> {
        if !self.connected {
            return Err(anyhow!("Not connected to signaling service"));
        }

        // Parse the connection link to extract the session ID
        let session_id = connection_link
            .strip_prefix("resonance://")
            .ok_or_else(|| anyhow!("Invalid connection link format"))?;

        // In a real implementation, this would connect to the session
        // and exchange information with other peers

        // Store the session ID
        self.session_id = Some(session_id.to_string());

        // Return the session info
        Ok(SessionInfo {
            session_id: session_id.to_string(),
            connection_link: connection_link.to_string(),
        })
    }

    /// Get the current session ID
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Check if connected to the signaling service
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Get a list of peers in the current session
    pub fn get_peers(&self) -> Vec<Peer> {
        let peers = self.peers.lock().unwrap();
        peers.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_signaling_connection() {
        let mut signaling = SignalingService::new();
        let result = signaling.connect().await;
        assert!(result.is_ok());
        assert!(signaling.is_connected());
    }

    #[tokio::test]
    async fn test_session_creation() {
        let mut signaling = SignalingService::new();
        signaling.connect().await.unwrap();

        let session_info = signaling.create_session().await.unwrap();
        assert!(!session_info.session_id.is_empty());
        assert!(!session_info.connection_link.is_empty());
        assert_eq!(
            signaling.session_id(),
            Some(session_info.session_id.as_str())
        );
    }

    #[tokio::test]
    async fn test_join_session() {
        let mut signaling1 = SignalingService::new();
        signaling1.connect().await.unwrap();
        let session_info = signaling1.create_session().await.unwrap();

        let mut signaling2 = SignalingService::new();
        signaling2.connect().await.unwrap();
        let join_result = signaling2.join_session(&session_info.connection_link).await;

        assert!(join_result.is_ok());
        assert_eq!(
            signaling2.session_id(),
            Some(session_info.session_id.as_str())
        );
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

        let result = signaling.disconnect().await;
        assert!(result.is_ok());
        assert!(!signaling.is_connected());
        assert_eq!(signaling.session_id(), None);
    }
}
