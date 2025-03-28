use anyhow::{anyhow, Result};
use std::sync::{Arc, Mutex};
use webrtc::api::{media_engine::MediaEngine, APIBuilder, API};
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::{configuration::RTCConfiguration, RTCPeerConnection};

/// Represents a WebRTC peer connection
pub struct PeerConnection {
    pc: Arc<RTCPeerConnection>,
    initialized: bool,
}

impl PeerConnection {
    fn new(pc: Arc<RTCPeerConnection>) -> Self {
        Self {
            pc,
            initialized: true,
        }
    }

    /// Check if the peer connection is initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Create an SDP offer to initiate a connection
    pub async fn create_offer(&self) -> Result<RTCSessionDescription> {
        let offer = self.pc.create_offer(None).await?;
        self.pc.set_local_description(offer.clone()).await?;
        Ok(offer)
    }

    /// Process an SDP answer from the remote peer
    pub async fn set_remote_answer(&self, answer: RTCSessionDescription) -> Result<()> {
        self.pc.set_remote_description(answer).await?;
        Ok(())
    }

    /// Create an SDP answer to respond to an offer
    pub async fn create_answer(&self) -> Result<RTCSessionDescription> {
        let answer = self.pc.create_answer(None).await?;
        self.pc.set_local_description(answer.clone()).await?;
        Ok(answer)
    }

    /// Process an SDP offer from the remote peer
    pub async fn set_remote_offer(&self, offer: RTCSessionDescription) -> Result<()> {
        self.pc.set_remote_description(offer).await?;
        Ok(())
    }
}

/// The WebRTC Manager handles the WebRTC functionality
pub struct WebRtcManager {
    api: Option<API>,
    peers: Mutex<Vec<Arc<PeerConnection>>>,
}

impl WebRtcManager {
    /// Create a new WebRTC manager
    pub fn new() -> Self {
        Self {
            api: None,
            peers: Mutex::new(Vec::new()),
        }
    }

    /// Initialize the WebRTC API
    pub fn initialize(&mut self) -> Result<()> {
        let media_engine = MediaEngine::default();

        // In a real implementation, we would register codecs here
        // media_engine.register_default_codecs()?;

        let api = APIBuilder::new().with_media_engine(media_engine).build();

        self.api = Some(api);
        Ok(())
    }

    /// Create a new peer connection
    pub async fn create_peer_connection(&self) -> Result<Arc<PeerConnection>> {
        let api = self
            .api
            .as_ref()
            .ok_or_else(|| anyhow!("WebRTC API not initialized"))?;

        // Configure the peer connection
        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_string()],
                ..Default::default()
            }],
            ..Default::default()
        };

        // Create the peer connection
        let pc = api.new_peer_connection(config).await?;

        // Create our wrapper
        let peer = Arc::new(PeerConnection::new(Arc::new(pc)));

        // Store it in our list of peers
        self.peers.lock().unwrap().push(peer.clone());

        Ok(peer)
    }

    /// Close all peer connections
    pub async fn close_all_connections(&self) -> Result<()> {
        let peers = self.peers.lock().unwrap().clone();

        for peer in peers {
            peer.pc.close().await?;
        }

        // Clear the list of peers
        self.peers.lock().unwrap().clear();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_webrtc_initialization() {
        let mut webrtc = WebRtcManager::new();
        assert!(webrtc.initialize().is_ok());
    }

    #[tokio::test]
    async fn test_webrtc_peer_creation() {
        let mut webrtc = WebRtcManager::new();
        webrtc.initialize().unwrap();

        let peer = webrtc.create_peer_connection().await;
        assert!(peer.is_ok());
        assert!(peer.unwrap().is_initialized());
    }
}
