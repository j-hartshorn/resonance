use serde::{Deserialize, Serialize};
use std;
use std::net::SocketAddr;
use thiserror::Error;
use uuid::Uuid;

/// Unique identifier for a peer in the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct PeerId(Uuid);

impl PeerId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for PeerId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for PeerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Display only the first 8 characters for brevity
        write!(f, "{}", &self.0.to_string()[..8])
    }
}

/// Unique identifier for a room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct RoomId(Uuid);

impl RoomId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create a RoomId from a UUID
    pub fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl Default for RoomId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for RoomId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unified error type for the application.
#[derive(Error, Debug)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    Config(String), // Placeholder for config crate errors

    #[error("Network error: {0}")]
    Network(String), // Placeholder for network crate errors

    #[error("Audio error: {0}")]
    Audio(String), // Placeholder for audio crate errors

    #[error("Cryptography error: {0}")]
    Crypto(String), // Placeholder for crypto crate errors

    #[error("Serialization error: {0}")]
    Serialization(String), // Placeholder for serialization errors

    #[error("Room logic error: {0}")]
    Room(String), // Placeholder for room logic errors

    #[error("Initialization error: {0}")]
    Initialization(String),

    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("Invalid state: {0}")]
    InvalidState(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error), // Catch-all for other errors
}

// Basic audio format definitions
pub const SAMPLE_RATE: u32 = 48000;
pub const CHANNELS: u16 = 1; // Start with Mono

/// Represents a buffer of audio samples.
/// Samples are typically f32.
pub type AudioBuffer = Vec<f32>;

/// Commands that can be sent to the room handler
#[derive(Debug, Clone)]
pub enum RoomCommand {
    /// Create a new room
    CreateRoom,

    /// Join a room via link
    JoinRoom {
        /// Room ID to join
        room_id: RoomId,
        /// Host address to connect to
        address: SocketAddr,
    },

    /// Approve a join request
    ApproveJoinRequest {
        /// ID of the peer to approve
        peer_id: PeerId,
    },

    /// Deny a join request
    DenyJoinRequest {
        /// ID of the peer to deny
        peer_id: PeerId,
        /// Optional reason for denial
        reason: Option<String>,
    },

    /// Disconnect from the room
    LeaveRoom,

    /// Request the current state of the room
    RequestState,

    /// Shutdown the handler (used for testing)
    Shutdown,
}

/// Commands that room sends to network
#[derive(Debug, Clone)]
pub enum NetworkCommand {
    /// Create a new room
    CreateRoom {
        /// Room ID to create
        room_id: RoomId,
    },

    /// Connect to an existing room
    ConnectToRoom {
        /// Room ID to connect to
        room_id: RoomId,
        /// Address to connect to
        address: SocketAddr,
    },

    /// Send a join response to a peer
    SendJoinResponse {
        /// ID of the peer to send response to
        peer_id: PeerId,
        /// Whether the join was approved
        approved: bool,
        /// Optional reason for rejection
        reason: Option<String>,
    },

    /// Initiate WebRTC connection with peer
    InitiateWebRtcConnection {
        /// ID of the peer to connect to
        peer_id: PeerId,
    },

    /// Handle received WebRTC SDP offer
    HandleWebRtcOffer {
        /// ID of the peer that sent the offer
        peer_id: PeerId,
        /// SDP offer as string
        offer: String,
    },

    /// Handle received WebRTC SDP answer
    HandleWebRtcAnswer {
        /// ID of the peer that sent the answer
        peer_id: PeerId,
        /// SDP answer as string
        answer: String,
    },

    /// Handle received WebRTC ICE candidate
    HandleWebRtcIceCandidate {
        /// ID of the peer that sent the ICE candidate
        peer_id: PeerId,
        /// ICE candidate as string
        candidate: String,
    },

    /// Send message via WebRTC data channel
    SendWebRtcDataChannelMessage {
        /// ID of the peer to send to
        peer_id: PeerId,
        /// Data channel label
        label: String,
        /// Message data
        data: Vec<u8>,
    },

    /// Disconnect from a peer
    DisconnectPeer {
        /// ID of the peer to disconnect from
        peer_id: PeerId,
    },
}

pub mod events;

// Re-export commonly used types from events
pub use events::{JoinRequestStatus, NetworkEvent, NetworkMessage, RoomEvent};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_id_display() {
        let peer_id = PeerId::new();
        let display = format!("{}", peer_id);
        assert_eq!(display.len(), 8);
        assert_eq!(display, &peer_id.0.to_string()[..8]);
    }

    #[test]
    fn room_id_display() {
        let room_id = RoomId::new();
        let display = format!("{}", room_id);
        assert_eq!(display, room_id.0.to_string());
    }

    #[test]
    fn peer_id_equality() {
        let id1 = PeerId::new();
        let id2 = PeerId(id1.0); // Same UUID
        let id3 = PeerId::new(); // Different UUID
        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn error_display() {
        let io_err = Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        assert!(format!("{}", io_err).contains("I/O error: file not found"));

        let config_err = Error::Config("Invalid setting".to_string());
        assert!(format!("{}", config_err).contains("Configuration error: Invalid setting"));

        let anyhow_err = Error::Other(anyhow::anyhow!("Something went wrong"));
        assert!(format!("{}", anyhow_err).contains("Something went wrong"));
    }
}
