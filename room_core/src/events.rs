use crate::{Error, PeerId};
use std::net::SocketAddr;

/// Network protocol message (simplified version)
/// In a real implementation, this would be more complex
#[derive(Debug, Clone)]
pub enum NetworkMessage {
    /// A basic text message
    Text(String),
    /// A binary message
    Binary(Vec<u8>),
}

/// Room events
#[derive(Debug, Clone, PartialEq)]
pub enum RoomEvent {
    /// A peer has been added to the room
    PeerAdded(PeerId),
    /// A peer has been removed from the room
    PeerRemoved(PeerId),
    /// A join request has been received
    JoinRequestReceived(PeerId),
    /// A join request status has changed
    JoinRequestStatusChanged(PeerId, JoinRequestStatus),
    /// The peer list has been updated
    PeerListUpdated,
}

/// Status of a join request
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinRequestStatus {
    Pending,
    Approved,
    Denied,
}

impl std::fmt::Display for JoinRequestStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JoinRequestStatus::Pending => write!(f, "Pending"),
            JoinRequestStatus::Approved => write!(f, "Approved"),
            JoinRequestStatus::Denied => write!(f, "Denied"),
        }
    }
}

/// Events emitted by the network subsystem to other parts of the application
#[derive(Debug, Clone)]
pub enum NetworkEvent {
    /// A new peer has connected
    PeerConnected {
        /// ID of the peer that connected
        peer_id: PeerId,
        /// Address of the peer
        address: SocketAddr,
    },

    /// A peer has disconnected
    PeerDisconnected {
        /// ID of the disconnected peer
        peer_id: PeerId,
        /// Optional reason for disconnection
        reason: Option<String>,
    },

    /// A message was received from a peer
    MessageReceived {
        /// ID of the peer that sent the message
        peer_id: PeerId,
        /// The message that was received
        message: NetworkMessage,
    },

    /// A peer requested to join a room
    JoinRequested {
        /// ID of the peer requesting to join
        peer_id: PeerId,
        /// Name of the peer
        name: String,
        /// Address of the peer
        address: SocketAddr,
    },

    /// A peer's join request was processed
    JoinResponseReceived {
        /// Whether the join was approved
        approved: bool,
        /// Optional reason for rejection
        reason: Option<String>,
    },

    /// Authentication with a peer failed
    AuthenticationFailed {
        /// Address of the peer
        address: SocketAddr,
        /// Reason for the failure
        reason: String,
    },

    /// Connection to a peer failed
    ConnectionFailed {
        /// Address of the peer we tried to connect to
        address: SocketAddr,
        /// Reason for the failure
        reason: String,
    },

    /// Authentication with a peer succeeded
    AuthenticationSucceeded {
        /// ID of the authenticated peer
        peer_id: PeerId,
    },

    /// A network error occurred
    Error {
        /// The error message
        message: String,
    },
}
