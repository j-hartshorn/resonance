// WebRTC integration module
// Manages WebRTC connections for audio streaming

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::api::API;
use webrtc::api::media_engine::MediaEngine;

use crate::app::config::Config;

/// Manages WebRTC connections for audio streaming
pub struct WebRtcManager {
    config: Config,
    api: API,
    peer_connections: Arc<Mutex<HashMap<String, Arc<RTCPeerConnection>>>>,
}

impl WebRtcManager {
    /// Create a new WebRTC manager
    pub async fn new(config: &Config) -> Result<Self> {
        // Create a MediaEngine
        let mut media_engine = MediaEngine::default();
        
        // Set up the WebRTC API
        let api = API::new();
        
        Ok(Self {
            config: config.clone(),
            api,
            peer_connections: Arc::new(Mutex::new(HashMap::new())),
        })
    }
    
    /// Create a new peer connection with a participant
    pub async fn create_peer_connection(&self, participant_id: &str) -> Result<Arc<RTCPeerConnection>> {
        // Convert ICE servers from the config
        let ice_servers: Vec<RTCIceServer> = self.config.network.ice_servers
            .iter()
            .map(|server| RTCIceServer {
                urls: vec![server.clone()],
                ..Default::default()
            })
            .collect();
        
        // Create WebRTC configuration
        let config = RTCConfiguration {
            ice_servers,
            ..Default::default()
        };
        
        // Create the peer connection
        let peer_connection = self.api.new_peer_connection(config).await?;
        
        // Store the peer connection
        let mut connections = self.peer_connections.lock().map_err(|_| anyhow!("Lock error"))?;
        connections.insert(participant_id.to_string(), Arc::clone(&peer_connection));
        
        Ok(peer_connection)
    }
    
    /// Close a peer connection
    pub async fn close_peer_connection(&self, participant_id: &str) -> Result<()> {
        let mut connections = self.peer_connections.lock().map_err(|_| anyhow!("Lock error"))?;
        
        if let Some(connection) = connections.remove(participant_id) {
            connection.close().await?;
        }
        
        Ok(())
    }
    
    /// Get a peer connection by participant ID
    pub fn get_peer_connection(&self, participant_id: &str) -> Result<Option<Arc<RTCPeerConnection>>> {
        let connections = self.peer_connections.lock().map_err(|_| anyhow!("Lock error"))?;
        
        Ok(connections.get(participant_id).cloned())
    }
    
    /// Close all peer connections
    pub async fn close_all(&self) -> Result<()> {
        let mut connections = self.peer_connections.lock().map_err(|_| anyhow!("Lock error"))?;
        
        for (_, connection) in connections.drain() {
            connection.close().await?;
        }
        
        Ok(())
    }
}