//! Room state management for room.rs
//!
//! This crate manages room state, including peer list,
//! join requests, and other stateful room operations.

use log::{debug, info};
use room_core::{Error, JoinRequestStatus, PeerId, RoomEvent, RoomId};
use std::collections::HashMap;
use std::fmt;

/// Maximum number of users allowed in a room
pub const MAX_USERS: usize = 8;

/// Information about a peer in the room
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// The peer's unique identifier
    pub id: PeerId,
    /// The peer's display name
    pub name: String,
    /// When the peer joined the room (as system time)
    pub joined_at: std::time::SystemTime,
}

/// Room state container
pub struct RoomState {
    /// The unique identifier for this room
    room_id: RoomId,
    /// Map of peers currently in the room
    peers: HashMap<PeerId, PeerInfo>,
    /// Map of pending join requests with their status
    pending_joins: HashMap<PeerId, JoinRequestStatus>,
}

impl RoomState {
    /// Create a new room with a random ID
    pub fn new() -> Self {
        Self {
            room_id: RoomId::new(),
            peers: HashMap::new(),
            pending_joins: HashMap::new(),
        }
    }

    /// Create a room with a specific ID
    pub fn with_id(room_id: RoomId) -> Self {
        Self {
            room_id,
            peers: HashMap::new(),
            pending_joins: HashMap::new(),
        }
    }

    /// Get the room ID
    pub fn id(&self) -> RoomId {
        self.room_id
    }

    /// Get a reference to the peers map
    pub fn peers(&self) -> &HashMap<PeerId, PeerInfo> {
        &self.peers
    }

    /// Get a reference to the pending joins map
    pub fn pending_joins(&self) -> &HashMap<PeerId, JoinRequestStatus> {
        &self.pending_joins
    }

    /// Add a peer to the room
    ///
    /// Returns an error if the room is full
    pub fn add_peer(&mut self, id: PeerId, name: String) -> Result<RoomEvent, Error> {
        // Check if the room is full
        if self.peers.len() >= MAX_USERS {
            return Err(Error::Room(format!(
                "Room is full (max {} users)",
                MAX_USERS
            )));
        }

        // Add the peer
        let peer_info = PeerInfo {
            id,
            name,
            joined_at: std::time::SystemTime::now(),
        };

        self.peers.insert(id, peer_info);
        info!("Added peer {} to room {}", id, self.room_id);

        Ok(RoomEvent::PeerAdded(id))
    }

    /// Remove a peer from the room
    ///
    /// Returns an error if the peer is not in the room
    pub fn remove_peer(&mut self, id: PeerId) -> Result<RoomEvent, Error> {
        if self.peers.remove(&id).is_none() {
            return Err(Error::NotFound(format!("Peer {} not found in room", id)));
        }

        info!("Removed peer {} from room {}", id, self.room_id);
        Ok(RoomEvent::PeerRemoved(id))
    }

    /// Handle a join request from a peer
    ///
    /// Adds the peer to the pending joins map with Pending status
    pub fn handle_join_request(&mut self, id: PeerId) -> Result<RoomEvent, Error> {
        // Check if the peer is already in the room
        if self.peers.contains_key(&id) {
            return Err(Error::InvalidState(format!(
                "Peer {} is already in the room",
                id
            )));
        }

        // Check if there's already a pending request
        if self.pending_joins.contains_key(&id) {
            return Err(Error::InvalidState(format!(
                "Peer {} already has a pending join request",
                id
            )));
        }

        // Add to pending joins
        self.pending_joins.insert(id, JoinRequestStatus::Pending);
        info!(
            "Received join request from peer {} for room {}",
            id, self.room_id
        );

        Ok(RoomEvent::JoinRequestReceived(id))
    }

    /// Approve a join request
    ///
    /// Returns an error if the request doesn't exist or the room is full
    pub fn approve_join_request(&mut self, id: PeerId) -> Result<RoomEvent, Error> {
        // Check if the request exists
        if !self.pending_joins.contains_key(&id) {
            return Err(Error::NotFound(format!(
                "No pending join request for peer {}",
                id
            )));
        }

        // Check if the room is full
        if self.peers.len() >= MAX_USERS {
            return Err(Error::Room(format!(
                "Cannot approve request: room is full (max {} users)",
                MAX_USERS
            )));
        }

        // Update the status
        self.pending_joins.insert(id, JoinRequestStatus::Approved);
        info!(
            "Approved join request from peer {} for room {}",
            id, self.room_id
        );

        Ok(RoomEvent::JoinRequestStatusChanged(
            id,
            JoinRequestStatus::Approved,
        ))
    }

    /// Deny a join request
    ///
    /// Returns an error if the request doesn't exist
    pub fn deny_join_request(&mut self, id: PeerId) -> Result<RoomEvent, Error> {
        // Check if the request exists
        if !self.pending_joins.contains_key(&id) {
            return Err(Error::NotFound(format!(
                "No pending join request for peer {}",
                id
            )));
        }

        // Update the status
        self.pending_joins.insert(id, JoinRequestStatus::Denied);
        info!(
            "Denied join request from peer {} for room {}",
            id, self.room_id
        );

        Ok(RoomEvent::JoinRequestStatusChanged(
            id,
            JoinRequestStatus::Denied,
        ))
    }

    /// Remove a join request (e.g., after handling an approved/denied request)
    pub fn remove_join_request(&mut self, id: PeerId) -> Result<(), Error> {
        if self.pending_joins.remove(&id).is_none() {
            return Err(Error::NotFound(format!("No join request for peer {}", id)));
        }

        debug!(
            "Removed join request for peer {} from room {}",
            id, self.room_id
        );
        Ok(())
    }

    /// Get the number of peers in the room
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Check if the room has space for more peers
    pub fn has_space(&self) -> bool {
        self.peers.len() < MAX_USERS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_peer_id() -> PeerId {
        PeerId::new()
    }

    #[test]
    fn create_room() {
        let room = RoomState::new();
        let id = room.id();
        assert_eq!(room.id(), id);
        assert_eq!(room.peers().len(), 0);
        assert_eq!(room.pending_joins().len(), 0);
    }

    #[test]
    fn create_room_with_id() {
        let room_id = RoomId::new();
        let room = RoomState::with_id(room_id);
        assert_eq!(room.id(), room_id);
    }

    #[test]
    fn test_add_peer() {
        let mut room = RoomState::new();
        let peer_id = create_test_peer_id();
        let name = "Test User".to_string();

        let event = room.add_peer(peer_id, name.clone()).unwrap();

        assert_eq!(event, RoomEvent::PeerAdded(peer_id));
        assert_eq!(room.peers().len(), 1);
        assert!(room.peers().contains_key(&peer_id));
        assert_eq!(room.peers()[&peer_id].name, name);
    }

    #[test]
    fn test_add_peer_room_full() {
        let mut room = RoomState::new();

        // Add MAX_USERS peers
        for i in 0..MAX_USERS {
            let peer_id = create_test_peer_id();
            let name = format!("User {}", i);
            room.add_peer(peer_id, name).unwrap();
        }

        // Try to add one more
        let peer_id = create_test_peer_id();
        let name = "One Too Many".to_string();
        let result = room.add_peer(peer_id, name);

        assert!(result.is_err());
        if let Err(Error::Room(msg)) = result {
            assert!(msg.contains("full"));
        } else {
            panic!("Expected Room error");
        }
    }

    #[test]
    fn test_remove_peer() {
        let mut room = RoomState::new();
        let peer_id = create_test_peer_id();

        // Add a peer
        room.add_peer(peer_id, "Test User".to_string()).unwrap();
        assert_eq!(room.peers().len(), 1);

        // Remove the peer
        let event = room.remove_peer(peer_id).unwrap();
        assert_eq!(event, RoomEvent::PeerRemoved(peer_id));
        assert_eq!(room.peers().len(), 0);
    }

    #[test]
    fn test_remove_nonexistent_peer() {
        let mut room = RoomState::new();
        let peer_id = create_test_peer_id();

        // Try to remove a peer that doesn't exist
        let result = room.remove_peer(peer_id);
        assert!(result.is_err());
        if let Err(Error::NotFound(_)) = result {
            // Expected
        } else {
            panic!("Expected NotFound error");
        }
    }

    #[test]
    fn test_handle_join_request() {
        let mut room = RoomState::new();
        let peer_id = create_test_peer_id();

        let event = room.handle_join_request(peer_id).unwrap();

        assert_eq!(event, RoomEvent::JoinRequestReceived(peer_id));
        assert_eq!(room.pending_joins().len(), 1);
        assert_eq!(room.pending_joins()[&peer_id], JoinRequestStatus::Pending);
    }

    #[test]
    fn test_handle_duplicate_join_request() {
        let mut room = RoomState::new();
        let peer_id = create_test_peer_id();

        // First request should succeed
        room.handle_join_request(peer_id).unwrap();

        // Second request should fail
        let result = room.handle_join_request(peer_id);
        assert!(result.is_err());
        if let Err(Error::InvalidState(_)) = result {
            // Expected
        } else {
            panic!("Expected InvalidState error");
        }
    }

    #[test]
    fn test_handle_join_request_for_existing_peer() {
        let mut room = RoomState::new();
        let peer_id = create_test_peer_id();

        // Add the peer
        room.add_peer(peer_id, "Test User".to_string()).unwrap();

        // Try to create a join request
        let result = room.handle_join_request(peer_id);
        assert!(result.is_err());
        if let Err(Error::InvalidState(_)) = result {
            // Expected
        } else {
            panic!("Expected InvalidState error");
        }
    }

    #[test]
    fn test_approve_join_request() {
        let mut room = RoomState::new();
        let peer_id = create_test_peer_id();

        // Create a join request
        room.handle_join_request(peer_id).unwrap();

        // Approve it
        let event = room.approve_join_request(peer_id).unwrap();

        assert_eq!(
            event,
            RoomEvent::JoinRequestStatusChanged(peer_id, JoinRequestStatus::Approved)
        );
        assert_eq!(room.pending_joins()[&peer_id], JoinRequestStatus::Approved);
    }

    #[test]
    fn test_approve_nonexistent_join_request() {
        let mut room = RoomState::new();
        let peer_id = create_test_peer_id();

        // Try to approve a request that doesn't exist
        let result = room.approve_join_request(peer_id);

        assert!(result.is_err());
        if let Err(Error::NotFound(_)) = result {
            // Expected
        } else {
            panic!("Expected NotFound error");
        }
    }

    #[test]
    fn test_approve_join_request_room_full() {
        let mut room = RoomState::new();
        let joiner_id = create_test_peer_id();

        // Create a join request
        room.handle_join_request(joiner_id).unwrap();

        // Fill the room
        for i in 0..MAX_USERS {
            let peer_id = create_test_peer_id();
            let name = format!("User {}", i);
            room.add_peer(peer_id, name).unwrap();
        }

        // Try to approve the request
        let result = room.approve_join_request(joiner_id);

        assert!(result.is_err());
        if let Err(Error::Room(msg)) = result {
            assert!(msg.contains("full"));
        } else {
            panic!("Expected Room error");
        }
    }

    #[test]
    fn test_deny_join_request() {
        let mut room = RoomState::new();
        let peer_id = create_test_peer_id();

        // Create a join request
        room.handle_join_request(peer_id).unwrap();

        // Deny it
        let event = room.deny_join_request(peer_id).unwrap();

        assert_eq!(
            event,
            RoomEvent::JoinRequestStatusChanged(peer_id, JoinRequestStatus::Denied)
        );
        assert_eq!(room.pending_joins()[&peer_id], JoinRequestStatus::Denied);
    }

    #[test]
    fn test_deny_nonexistent_join_request() {
        let mut room = RoomState::new();
        let peer_id = create_test_peer_id();

        // Try to deny a request that doesn't exist
        let result = room.deny_join_request(peer_id);

        assert!(result.is_err());
        if let Err(Error::NotFound(_)) = result {
            // Expected
        } else {
            panic!("Expected NotFound error");
        }
    }

    #[test]
    fn test_remove_join_request() {
        let mut room = RoomState::new();
        let peer_id = create_test_peer_id();

        // Create a join request
        room.handle_join_request(peer_id).unwrap();
        assert_eq!(room.pending_joins().len(), 1);

        // Remove it
        room.remove_join_request(peer_id).unwrap();
        assert_eq!(room.pending_joins().len(), 0);
    }

    #[test]
    fn test_remove_nonexistent_join_request() {
        let mut room = RoomState::new();
        let peer_id = create_test_peer_id();

        // Try to remove a request that doesn't exist
        let result = room.remove_join_request(peer_id);

        assert!(result.is_err());
        if let Err(Error::NotFound(_)) = result {
            // Expected
        } else {
            panic!("Expected NotFound error");
        }
    }

    #[test]
    fn test_peer_count_and_has_space() {
        let mut room = RoomState::new();

        assert_eq!(room.peer_count(), 0);
        assert!(room.has_space());

        // Add MAX_USERS - 1 peers
        for i in 0..(MAX_USERS - 1) {
            let peer_id = create_test_peer_id();
            let name = format!("User {}", i);
            room.add_peer(peer_id, name).unwrap();
        }

        assert_eq!(room.peer_count(), MAX_USERS - 1);
        assert!(room.has_space());

        // Add one more peer
        let peer_id = create_test_peer_id();
        room.add_peer(peer_id, "Last User".to_string()).unwrap();

        assert_eq!(room.peer_count(), MAX_USERS);
        assert!(!room.has_space());
    }

    #[test]
    fn test_join_request_status_display() {
        assert_eq!(format!("{}", JoinRequestStatus::Pending), "Pending");
        assert_eq!(format!("{}", JoinRequestStatus::Approved), "Approved");
        assert_eq!(format!("{}", JoinRequestStatus::Denied), "Denied");
    }
}

// Add module declarations
pub mod commands;
pub mod handler;
