// Export all necessary modules
pub mod connection_manager;
pub mod p2p;
mod secure_channel;
mod security;
mod signaling;
mod webrtc;

// Re-export necessary components
pub use connection_manager::ConnectionManager;
pub use p2p::{
    discover_public_endpoint, establish_direct_udp_connection, generate_connection_link,
    is_blocked_ip, parse_connection_link, ConnectionState, Endpoint,
};
pub use secure_channel::{Keypair, Message, SecureChannel};
pub use security::SecurityModule;
pub use signaling::{Peer, SessionInfo, SignalingInterface, SignalingService};
pub use webrtc::{PeerConnection, WebRtcManager};

// Networking errors
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Invalid parameters: {0}")]
    InvalidParameters(String),

    #[error("Security error: {0}")]
    SecurityError(String),

    #[error("Connection lost: {0}")]
    ConnectionLost(String),

    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),
}
