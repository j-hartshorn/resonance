use log::{debug, error, info, warn};
use network::NetworkManager;
use room::handler::RoomHandler;
use room_core::{NetworkCommand, NetworkEvent, PeerId, RoomCommand, RoomEvent, RoomId};
use std::net::SocketAddr;
use tokio::sync::mpsc;

/// Network adapter connects the UI, room, and network components
pub struct NetworkAdapter {
    /// Our peer ID
    peer_id: PeerId,
    /// Channel for sending room commands
    room_cmd_tx: mpsc::Sender<RoomCommand>,
    /// Channel for receiving room events
    room_event_rx: mpsc::Receiver<RoomEvent>,
}

impl NetworkAdapter {
    /// Create a new network adapter
    pub async fn new() -> Self {
        let peer_id = PeerId::new();

        // Create channels
        let (room_cmd_tx, room_cmd_rx) = mpsc::channel(100);
        let (network_cmd_tx, network_cmd_rx) = mpsc::channel(100);
        let (network_event_tx, network_event_rx) = mpsc::channel(100);
        let (room_event_tx, room_event_rx) = mpsc::channel(100);

        // Create NetworkManager with random port binding for testing
        let bind_addr = SocketAddr::new(
            "127.0.0.1".parse().unwrap(),
            0, // Use port 0 to get a random available port
        );

        let mut network_manager = match NetworkManager::new(peer_id).await {
            Ok(manager) => manager,
            Err(e) => {
                error!("Failed to create network manager: {}", e);
                panic!("Failed to create network manager: {}", e);
            }
        };

        // Create RoomHandler
        let mut room_handler = RoomHandler::new(
            peer_id,
            room_cmd_rx,
            network_cmd_tx,
            network_event_rx,
            room_event_tx,
        );

        // Start NetworkManager in a background task
        tokio::spawn(async move {
            if let Err(e) = network_manager.run().await {
                error!("Network manager error: {}", e);
            }
        });

        // Start RoomHandler in a background task
        tokio::spawn(async move {
            if let Err(e) = room_handler.run().await {
                error!("Room handler error: {}", e);
            }
        });

        // Return the adapter with channels for the UI to use
        Self {
            peer_id,
            room_cmd_tx,
            room_event_rx,
        }
    }

    /// Create a new room
    pub async fn create_room(&self) -> Result<(), mpsc::error::SendError<RoomCommand>> {
        self.room_cmd_tx.send(RoomCommand::CreateRoom).await
    }

    /// Join a room using a link
    /// The link format is expected to be "room:<room_id>@<host>:<port>"
    pub async fn join_room(&self, link: &str) -> Result<(), String> {
        // Parse the link
        if !link.starts_with("room:") {
            return Err("Invalid link format, must start with 'room:'".to_string());
        }

        let link = &link[5..]; // Remove "room:" prefix

        // Split at @ to get room_id and address
        let parts: Vec<&str> = link.split('@').collect();
        if parts.len() != 2 {
            return Err("Invalid link format, missing '@' separator".to_string());
        }

        // Parse room ID
        let room_id_str = parts[0];
        let room_id = match uuid::Uuid::parse_str(room_id_str) {
            Ok(uuid) => RoomId::from(uuid),
            Err(_) => return Err(format!("Invalid room ID: {}", room_id_str)),
        };

        // Parse address
        let address: SocketAddr = match parts[1].parse() {
            Ok(addr) => addr,
            Err(_) => return Err(format!("Invalid address: {}", parts[1])),
        };

        // Send join command
        match self
            .room_cmd_tx
            .send(RoomCommand::JoinRoom { room_id, address })
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed to send join command: {}", e)),
        }
    }

    /// Approve a join request
    pub async fn approve_join_request(
        &self,
        peer_id: PeerId,
    ) -> Result<(), mpsc::error::SendError<RoomCommand>> {
        self.room_cmd_tx
            .send(RoomCommand::ApproveJoinRequest { peer_id })
            .await
    }

    /// Deny a join request
    pub async fn deny_join_request(
        &self,
        peer_id: PeerId,
        reason: Option<String>,
    ) -> Result<(), mpsc::error::SendError<RoomCommand>> {
        self.room_cmd_tx
            .send(RoomCommand::DenyJoinRequest { peer_id, reason })
            .await
    }

    /// Leave the current room
    pub async fn leave_room(&self) -> Result<(), mpsc::error::SendError<RoomCommand>> {
        self.room_cmd_tx.send(RoomCommand::LeaveRoom).await
    }

    /// Request current room state
    pub async fn request_state(&self) -> Result<(), mpsc::error::SendError<RoomCommand>> {
        self.room_cmd_tx.send(RoomCommand::RequestState).await
    }

    /// Try to receive a room event (non-blocking)
    pub async fn try_recv_event(&mut self) -> Option<RoomEvent> {
        match self.room_event_rx.try_recv() {
            Ok(event) => Some(event),
            Err(_) => None,
        }
    }

    /// Get our peer ID
    pub fn peer_id(&self) -> PeerId {
        self.peer_id
    }
}
