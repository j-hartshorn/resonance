//! Network communication for room.rs
//!
//! This crate handles all networking for the application
//! from basic UDP sockets to WebRTC connection management.

use log::{debug, error, info, trace, warn};
use room_core::{Error, PeerId, RoomId};
use std::net::SocketAddr;
use tokio::sync::mpsc;

pub mod events;
pub mod phase1;
pub mod protocol;

use events::NetworkEvent;
use phase1::Phase1Network;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_network_manager() {
        let peer_id = PeerId::new();
        let manager = NetworkManager::new(peer_id).await;
        assert!(manager.is_ok());

        let manager = manager.unwrap();
        assert_eq!(manager.peer_id, peer_id);
        assert_eq!(manager.room_id, None);
    }

    // Additional tests would follow for room creation, connection, etc.

    pub mod phase1_test;
}

/// Network manager coordinates all networking operations
pub struct NetworkManager {
    /// Our peer ID
    peer_id: PeerId,
    /// Current room ID
    room_id: Option<RoomId>,
    /// Phase 1 network (UDP-based secure channel)
    phase1: Phase1Network,
    /// Channel for sending network events
    event_tx: mpsc::Sender<NetworkEvent>,
    /// Channel for receiving network events (for internal forwarding)
    _event_rx: mpsc::Receiver<NetworkEvent>,
}

impl NetworkManager {
    /// Create a new network manager
    pub async fn new(peer_id: PeerId) -> Result<Self, Error> {
        // Create channels for network events
        let (event_tx, event_rx) = mpsc::channel(100);

        // Create Phase1Network
        let phase1 = Phase1Network::new(peer_id, None, event_tx.clone()).await?;

        Ok(Self {
            peer_id,
            room_id: None,
            phase1,
            event_tx,
            _event_rx: event_rx,
        })
    }

    /// Start the network manager
    pub async fn start(&self) -> Result<(), Error> {
        // Start the Phase1Network
        self.phase1.start().await?;

        Ok(())
    }

    /// Connect to a room using a remote address
    pub async fn connect_to_room(
        &mut self,
        room_id: RoomId,
        address: SocketAddr,
    ) -> Result<(), Error> {
        self.room_id = Some(room_id);

        // Connect using Phase1Network
        self.phase1.connect(room_id, address).await?;

        Ok(())
    }

    /// Create a new room
    pub async fn create_room(&mut self, room_id: RoomId) -> Result<(), Error> {
        self.room_id = Some(room_id);

        // Create room in Phase1Network
        self.phase1.create_room(room_id).await?;

        Ok(())
    }

    /// Send a join response to a peer
    pub async fn send_join_response(
        &self,
        peer_id: PeerId,
        approved: bool,
        reason: Option<String>,
    ) -> Result<(), Error> {
        // Send via Phase1Network
        self.phase1
            .send_join_response(peer_id, approved, reason)
            .await
    }

    /// Get a clone of the event sender
    pub fn get_event_sender(&self) -> mpsc::Sender<NetworkEvent> {
        self.event_tx.clone()
    }

    /// Get the current peers in the room
    pub async fn get_peers(&self) -> Result<Vec<protocol::PeerInfo>, Error> {
        // Get peers from Phase1Network
        Ok(self.phase1.get_peers().await)
    }

    /// Get the current room ID
    pub fn get_room_id(&self) -> Option<RoomId> {
        self.room_id
    }

    /// Disconnect from a peer
    pub async fn disconnect_peer(&self, peer_id: PeerId) -> Result<(), Error> {
        // Disconnect via Phase1Network
        self.phase1.disconnect_peer(peer_id).await
    }
}
