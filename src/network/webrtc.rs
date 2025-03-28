use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid;
use webrtc::api::{media_engine::MediaEngine, APIBuilder, API};
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::{configuration::RTCConfiguration, RTCPeerConnection};

/// Wrapper around a WebRTC peer connection with additional metadata
#[derive(Clone)]
pub struct PeerConnection {
    /// The underlying WebRTC connection
    connection: Arc<RTCPeerConnection>,
    /// Unique identifier for this connection
    id: String,
    /// Session ID this connection belongs to
    session_id: String,
}

impl PeerConnection {
    /// Creates a new peer connection wrapper
    fn new(connection: Arc<RTCPeerConnection>, id: String, session_id: String) -> Self {
        Self {
            connection,
            id,
            session_id,
        }
    }

    /// Checks if the connection is initialized
    pub fn is_initialized(&self) -> bool {
        true
    }

    /// Creates an SDP offer for this connection
    pub async fn create_offer(&self) -> Result<RTCSessionDescription> {
        let offer = self.connection.create_offer(None).await?;
        self.connection.set_local_description(offer.clone()).await?;
        Ok(offer)
    }

    /// Sets a remote SDP answer
    pub async fn set_remote_answer(&self, answer: RTCSessionDescription) -> Result<()> {
        self.connection.set_remote_description(answer).await?;
        Ok(())
    }

    /// Creates an SDP answer for this connection
    pub async fn create_answer(&self) -> Result<RTCSessionDescription> {
        let answer = self.connection.create_answer(None).await?;
        self.connection
            .set_local_description(answer.clone())
            .await?;
        Ok(answer)
    }

    /// Sets a remote SDP offer
    pub async fn set_remote_offer(&self, offer: RTCSessionDescription) -> Result<()> {
        self.connection.set_remote_description(offer).await?;
        Ok(())
    }

    /// Returns the session ID for this connection
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Returns the unique ID for this connection
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// Manages WebRTC connections for audio communication
pub struct WebRtcManager {
    api: Option<webrtc::api::API>,
    connections: Arc<Mutex<HashMap<String, PeerConnection>>>,
}

impl Clone for WebRtcManager {
    fn clone(&self) -> Self {
        // We can't clone the API, so we create a new instance with a None value
        Self {
            api: None,
            connections: Arc::clone(&self.connections),
        }
    }
}

impl WebRtcManager {
    /// Creates a new WebRTC manager
    pub fn new() -> Self {
        Self {
            api: None,
            connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Initializes the WebRTC API
    pub fn initialize(&mut self) -> Result<()> {
        let media_engine = MediaEngine::default();

        // In a real implementation, we would register codecs here
        // media_engine.register_default_codecs()?;

        let api = APIBuilder::new().with_media_engine(media_engine).build();

        self.api = Some(api);
        Ok(())
    }

    /// Creates a new WebRTC peer connection
    pub async fn create_peer_connection(&self, session_id: String) -> Result<PeerConnection> {
        let api = self
            .api
            .as_ref()
            .ok_or_else(|| anyhow!("WebRTC API not initialized"))?;

        // Configure ICE servers (STUN/TURN)
        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_string()],
                ..Default::default()
            }],
            ..Default::default()
        };

        // Create the peer connection
        let peer_connection = api.new_peer_connection(config).await?;

        // Generate a unique ID for this connection
        let conn_id = uuid::Uuid::new_v4().to_string();

        // Wrap the peer connection
        let connection = PeerConnection::new(
            Arc::new(peer_connection),
            conn_id.clone(),
            session_id.clone(),
        );

        // Store the connection
        self.connections
            .lock()
            .unwrap()
            .insert(conn_id, connection.clone());

        Ok(connection)
    }

    /// Gets all active connections
    pub fn get_connections(&self) -> Result<Vec<PeerConnection>> {
        let connections = self.connections.lock().unwrap();
        let result = connections.values().cloned().collect();
        Ok(result)
    }

    /// Gets connections for a specific session
    pub fn get_session_connections(&self, session_id: &str) -> Result<Vec<PeerConnection>> {
        let connections = self.connections.lock().unwrap();
        let result = connections
            .values()
            .filter(|conn| conn.session_id() == session_id)
            .cloned()
            .collect();
        Ok(result)
    }

    /// Closes a specific connection
    pub async fn close_connection(&self, conn_id: &str) -> Result<()> {
        let mut connections = self.connections.lock().unwrap();

        if let Some(connection) = connections.remove(conn_id) {
            connection.connection.close().await?;
        }

        Ok(())
    }

    /// Closes all connections
    pub async fn close_all_connections(&self) -> Result<()> {
        let conn_ids: Vec<String> = {
            let connections = self.connections.lock().unwrap();
            connections.keys().cloned().collect()
        };

        for id in conn_ids {
            self.close_connection(&id).await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webrtc_initialization() {
        let mut webrtc = WebRtcManager::new();
        assert!(webrtc.initialize().is_ok());
    }

    #[tokio::test]
    async fn test_webrtc_peer_creation() {
        let mut webrtc = WebRtcManager::new();
        webrtc.initialize().unwrap();

        let session_id = "test-session".to_string();
        let peer = webrtc
            .create_peer_connection(session_id.clone())
            .await
            .unwrap();

        assert!(peer.is_initialized());
        assert_eq!(peer.session_id(), session_id);

        // Test getting connections
        let connections = webrtc.get_connections().unwrap();
        assert_eq!(connections.len(), 1);

        // Test getting connections by session
        let session_connections = webrtc.get_session_connections(&session_id).unwrap();
        assert_eq!(session_connections.len(), 1);

        // Test connection cleanup
        webrtc.close_all_connections().await.unwrap();
        let connections = webrtc.get_connections().unwrap();
        assert_eq!(connections.len(), 0);
    }
}
