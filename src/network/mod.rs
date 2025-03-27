// Network communication module
// Handles WebRTC connections and signaling

pub mod webrtc;
pub mod signaling;
pub mod security;

use anyhow::Result;
use crate::app::config::Config;
use crate::network::webrtc::WebRtcManager;

/// Set up the network subsystem
pub async fn setup(config: &Config) -> Result<WebRtcManager> {
    // Initialize WebRTC
    let webrtc_manager = WebRtcManager::new(config).await?;
    
    // Initialize security module
    security::init()?;
    
    Ok(webrtc_manager)
}