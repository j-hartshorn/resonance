//! Room state management for room.rs
//!
//! This crate manages room state, including peer list,
//! join requests, and other stateful room operations.

use room_core::{Error, PeerId, RoomId};
use log::{debug, error, info, trace, warn};

/// Room state container
pub struct RoomState {
    // TODO: Implement in Phase 4
    room_id: RoomId,
}

impl RoomState {
    /// Create a new room with a random ID
    pub fn new() -> Self {
        Self {
            room_id: RoomId::new(),
        }
    }

    /// Create a room with a specific ID
    pub fn with_id(room_id: RoomId) -> Self {
        Self { room_id }
    }

    /// Get the room ID
    pub fn id(&self) -> RoomId {
        self.room_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_room() {
        let room = RoomState::new();
        let id = room.id();
        assert_eq!(room.id(), id);
    }
}
