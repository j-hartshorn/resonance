use room_core::{Error, PeerId, RoomId};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use x25519_dalek::PublicKey;

/// Maximum size of a UDP payload we expect to handle
pub const MAX_UDP_PAYLOAD_SIZE: usize = 1400;

/// Protocol version used for compatibility checks
pub const PROTOCOL_VERSION: u8 = 1;

/// Application layer messages carried in encrypted Phase1 payloads
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ApplicationMessage {
    /// WebRTC SDP offer for connection establishment
    SdpOffer {
        /// The SDP offer as a string
        offer: String,
    },

    /// WebRTC SDP answer in response to an offer
    SdpAnswer {
        /// The SDP answer as a string
        answer: String,
    },

    /// WebRTC ICE candidate for connection negotiation
    IceCandidate {
        /// JSON representation of the ICE candidate
        candidate: String,
    },

    /// Request for a peer's current peer list (for mesh healing)
    PeerListRequest,

    /// Response containing a list of known peers
    PeerListResponse {
        /// List of peers currently in the room
        peers: Vec<PeerInfo>,
    },
}

/// Phase 1 messages for initial connection bootstrapping and secure channel setup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Phase1Message {
    /// Initial hello message to establish contact with a server/peer
    HelloInitiate {
        /// Protocol version
        version: u8,
        /// ID of the room this peer wants to join
        room_id: RoomId,
        /// Unique ID of the initiating peer
        peer_id: PeerId,
    },

    /// Response acknowledging the initial hello
    HelloAck {
        /// Protocol version
        version: u8,
        /// ID of the room being joined
        room_id: RoomId,
        /// ID of the responding peer
        peer_id: PeerId,
    },

    /// Message containing a Diffie-Hellman public key
    DHPubKey {
        /// The sender's public key
        pub_key: [u8; crypto::DH_PUBLIC_KEY_SIZE],
    },

    /// Authentication tag for verifying the Diffie-Hellman exchange
    AuthTag {
        /// HMAC tag that authenticates the DH exchange
        tag: [u8; crypto::HMAC_SIZE],
    },

    /// Request to join a room (sent after secure channel established)
    JoinRequest {
        /// The peer requesting to join
        peer_id: PeerId,
        /// Name/nickname of the peer
        name: String,
    },

    /// Response to a join request
    JoinResponse {
        /// Whether the join was approved
        approved: bool,
        /// Optional reason for rejection
        reason: Option<String>,
    },

    /// Encrypted payload for application messages
    EncryptedMessage {
        /// The encrypted data (includes nonce)
        payload: Vec<u8>,
    },

    /// Application layer message (after secure channel established)
    /// Used for WebRTC signaling and other application-level communication
    ApplicationMessage {
        /// The application message
        message: ApplicationMessage,
    },

    /// Ping message to keep connections alive
    Ping {
        /// The ID of the peer sending the ping
        peer_id: PeerId,
    },

    /// Response to a ping
    Pong {
        /// The ID of the peer responding to the ping
        peer_id: PeerId,
    },
}

/// Represents a peer's connection information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    /// The peer's unique ID
    pub peer_id: PeerId,
    /// The socket address of the peer
    pub address: SocketAddr,
    /// The peer's display name
    pub name: Option<String>,
}

/// Serialize a message to bytes using bincode
pub fn serialize(message: &Phase1Message) -> Result<Vec<u8>, Error> {
    bincode::serialize(message)
        .map_err(|e| Error::Serialization(format!("Failed to serialize message: {}", e)))
}

/// Deserialize bytes to a message using bincode
pub fn deserialize(bytes: &[u8]) -> Result<Phase1Message, Error> {
    bincode::deserialize(bytes)
        .map_err(|e| Error::Serialization(format!("Failed to deserialize message: {}", e)))
}

/// Serialize an application message to bytes
pub fn serialize_application_message(message: &ApplicationMessage) -> Result<Vec<u8>, Error> {
    bincode::serialize(message).map_err(|e| {
        Error::Serialization(format!("Failed to serialize application message: {}", e))
    })
}

/// Deserialize bytes to an application message
pub fn deserialize_application_message(bytes: &[u8]) -> Result<ApplicationMessage, Error> {
    bincode::deserialize(bytes).map_err(|e| {
        Error::Serialization(format!("Failed to deserialize application message: {}", e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let original = Phase1Message::HelloInitiate {
            version: PROTOCOL_VERSION,
            room_id: RoomId::new(),
            peer_id: PeerId::new(),
        };

        let serialized = serialize(&original).expect("Serialization failed");
        let deserialized = deserialize(&serialized).expect("Deserialization failed");

        match (original, deserialized) {
            (
                Phase1Message::HelloInitiate {
                    version: v1,
                    room_id: r1,
                    peer_id: p1,
                },
                Phase1Message::HelloInitiate {
                    version: v2,
                    room_id: r2,
                    peer_id: p2,
                },
            ) => {
                assert_eq!(v1, v2);
                assert_eq!(r1, r2);
                assert_eq!(p1, p2);
            }
            _ => panic!("Deserialized message does not match original"),
        }
    }

    #[test]
    fn test_application_message_roundtrip() {
        let original = ApplicationMessage::SdpOffer {
            offer: "test sdp offer".to_string(),
        };

        let serialized = serialize_application_message(&original).expect("Serialization failed");
        let deserialized =
            deserialize_application_message(&serialized).expect("Deserialization failed");

        match (original, deserialized) {
            (
                ApplicationMessage::SdpOffer { offer: o1 },
                ApplicationMessage::SdpOffer { offer: o2 },
            ) => {
                assert_eq!(o1, o2);
            }
            _ => panic!("Deserialized message does not match original"),
        }
    }
}
