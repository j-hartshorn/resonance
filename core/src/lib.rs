use serde::{Deserialize, Serialize};
use std;
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
