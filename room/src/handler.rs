use crate::commands::{NetworkCommand, RoomCommand};
use crate::{RoomEvent, RoomState};
use log::{debug, error, info, warn};
use room_core::{Error, NetworkEvent, PeerId, RoomId};
use std::net::SocketAddr;
use tokio::sync::mpsc;

/// Handler for room operations, coordinating between the UI and network
pub struct RoomHandler {
    /// The room state
    state: RoomState,
    /// Our peer ID
    peer_id: PeerId,
    /// Channel for receiving room commands from the UI
    command_rx: mpsc::Receiver<RoomCommand>,
    /// Channel for sending network commands
    network_tx: mpsc::Sender<NetworkCommand>,
    /// Channel for receiving network events
    network_rx: mpsc::Receiver<NetworkEvent>,
    /// Channel for sending room events to the UI
    event_tx: mpsc::Sender<RoomEvent>,
}

impl RoomHandler {
    /// Create a new room handler
    pub fn new(
        peer_id: PeerId,
        command_rx: mpsc::Receiver<RoomCommand>,
        network_tx: mpsc::Sender<NetworkCommand>,
        network_rx: mpsc::Receiver<NetworkEvent>,
        event_tx: mpsc::Sender<RoomEvent>,
    ) -> Self {
        Self {
            state: RoomState::new(),
            peer_id,
            command_rx,
            network_tx,
            network_rx,
            event_tx,
        }
    }

    /// Run the room handler, processing commands and events
    pub async fn run(&mut self) -> Result<(), Error> {
        loop {
            tokio::select! {
                // Process commands from the UI
                Some(command) = self.command_rx.recv() => {
                    // Check for shutdown command first
                    if let RoomCommand::Shutdown = command {
                        info!("Received shutdown command, exiting handler");
                        break;
                    }

                    if let Err(e) = self.handle_command(command).await {
                        error!("Error handling room command: {}", e);
                        // Send error to UI?
                    }
                }

                // Process events from the network
                Some(event) = self.network_rx.recv() => {
                    if let Err(e) = self.handle_network_event(event).await {
                        error!("Error handling network event: {}", e);
                        // Send error to UI?
                    }
                }

                // Check for task cancellation
                else => break,
            }
        }

        Ok(())
    }

    /// Handle a command from the UI
    async fn handle_command(&mut self, command: RoomCommand) -> Result<(), Error> {
        match command {
            RoomCommand::CreateRoom => {
                info!("Creating a new room");
                let room_id = RoomId::new();

                // Update room state
                self.state = RoomState::with_id(room_id);

                // Send command to network
                self.network_tx
                    .send(NetworkCommand::CreateRoom { room_id })
                    .await
                    .map_err(|e| {
                        Error::Network(format!("Failed to send CreateRoom command: {}", e))
                    })?;

                // Add ourselves to the room
                let event = self.state.add_peer(self.peer_id, "You".to_string())?;
                self.emit_event(event).await?;

                // Notify UI of room creation
                self.emit_event(RoomEvent::PeerListUpdated).await?;
            }

            RoomCommand::JoinRoom { room_id, address } => {
                info!("Joining room {} at {}", room_id, address);

                // Update room state
                self.state = RoomState::with_id(room_id);

                // Send command to network
                self.network_tx
                    .send(NetworkCommand::ConnectToRoom { room_id, address })
                    .await
                    .map_err(|e| {
                        Error::Network(format!("Failed to send ConnectToRoom command: {}", e))
                    })?;

                // Add ourselves to the room
                let event = self.state.add_peer(self.peer_id, "You".to_string())?;
                self.emit_event(event).await?;

                // Notify UI of pending connection
                self.emit_event(RoomEvent::PeerListUpdated).await?;
            }

            RoomCommand::ApproveJoinRequest { peer_id } => {
                info!("Approving join request from peer {}", peer_id);

                // Update room state
                let event = self.state.approve_join_request(peer_id)?;
                self.emit_event(event).await?;

                // Send approval message via Phase 1 channel
                self.network_tx
                    .send(NetworkCommand::SendJoinResponse {
                        peer_id,
                        approved: true,
                        reason: None,
                    })
                    .await
                    .map_err(|e| Error::Network(format!("Failed to send join response: {}", e)))?;

                // Initiate WebRTC connection (using the correct variant)
                self.network_tx
                    .send(NetworkCommand::InitiateWebRtcConnection { peer_id })
                    .await
                    .map_err(|e| {
                        Error::Network(format!(
                            "Failed to send InitiateWebRtcConnection command: {}",
                            e
                        ))
                    })?;
            }

            RoomCommand::DenyJoinRequest { peer_id, reason } => {
                info!("Denying join request from peer {}", peer_id);

                // Update room state
                let event = self.state.deny_join_request(peer_id)?;
                self.emit_event(event).await?;

                // Send command to network
                self.network_tx
                    .send(NetworkCommand::SendJoinResponse {
                        peer_id,
                        approved: false,
                        reason,
                    })
                    .await
                    .map_err(|e| Error::Network(format!("Failed to send join response: {}", e)))?;
            }

            RoomCommand::LeaveRoom => {
                info!("Leaving room {}", self.state.id());

                // Disconnect from all peers
                for peer_id in self.state.peers().keys().copied().collect::<Vec<_>>() {
                    if peer_id != self.peer_id {
                        // Don't disconnect from ourselves
                        self.network_tx
                            .send(NetworkCommand::DisconnectPeer { peer_id })
                            .await
                            .map_err(|e| {
                                Error::Network(format!("Failed to send disconnect command: {}", e))
                            })?;
                    }
                }

                // Reset room state
                self.state = RoomState::new();
                self.emit_event(RoomEvent::PeerListUpdated).await?;
            }

            RoomCommand::RequestState => {
                debug!("Sending current room state to UI");
                self.emit_event(RoomEvent::PeerListUpdated).await?;
            }

            RoomCommand::Shutdown => {
                // This is handled in the run method before we reach here
                unreachable!()
            }
        }

        Ok(())
    }

    /// Handle an event from the network
    async fn handle_network_event(&mut self, event: NetworkEvent) -> Result<(), Error> {
        match event {
            NetworkEvent::PeerConnected { peer_id, address } => {
                info!("Peer {} connected from {}", peer_id, address);

                // For now, we'll just add the peer with a generic name
                // In a real implementation, we'd get the name from the connection process
                let name = format!("Peer {}", peer_id);
                let event = self.state.add_peer(peer_id, name)?;
                self.emit_event(event).await?;
                self.emit_event(RoomEvent::PeerListUpdated).await?;
            }

            NetworkEvent::PeerDisconnected { peer_id, reason } => {
                info!(
                    "Peer {} disconnected: {}",
                    peer_id,
                    reason.as_deref().unwrap_or("No reason provided")
                );

                if let Ok(event) = self.state.remove_peer(peer_id) {
                    self.emit_event(event).await?;
                    self.emit_event(RoomEvent::PeerListUpdated).await?;
                }
            }

            NetworkEvent::JoinRequested {
                peer_id,
                name,
                address,
            } => {
                info!("Join request from {} ({}) at {}", peer_id, name, address);

                // Add to pending joins
                let event = self.state.handle_join_request(peer_id)?;
                self.emit_event(event).await?;
            }

            NetworkEvent::JoinResponseReceived { approved, reason } => {
                info!(
                    "Join response received: {} {}",
                    if approved { "Approved" } else { "Denied" },
                    reason.as_deref().unwrap_or("")
                );

                // If approved, we're already in the room
                // If denied, we should reset our state
                if !approved {
                    self.state = RoomState::new();
                    self.emit_event(RoomEvent::PeerListUpdated).await?;
                }
            }

            NetworkEvent::MessageReceived { peer_id, message } => {
                debug!("Received message from {}: {:?}", peer_id, message);
                // Process Phase 1 messages if needed
            }

            NetworkEvent::AuthenticationFailed { address, reason } => {
                warn!("Authentication failed with {}: {}", address, reason);
                // Handle auth failure
            }

            NetworkEvent::ConnectionFailed { address, reason } => {
                warn!("Connection failed to {}: {}", address, reason);
                // Handle connection failure
            }

            NetworkEvent::AuthenticationSucceeded { peer_id } => {
                info!("Authentication succeeded with peer {}", peer_id);
                // Handle successful auth
            }

            NetworkEvent::Error { message } => {
                error!("Network error: {}", message);
                // Handle network error
            }

            // Update to use RoomState's methods for WebRTC status
            NetworkEvent::WebRtcConnectionStateChanged { peer_id, state } => {
                debug!(
                    "WebRTC connection state changed for peer {}: {}",
                    peer_id, state
                );

                // If the connection state is "Connected", we can consider the peer fully joined
                if state == "Connected" {
                    debug!("WebRTC connected to peer {}", peer_id);
                    // Mark the peer as connected
                    let _ = self.state.update_webrtc_status(peer_id, true);
                } else if state == "Failed" || state == "Closed" || state == "Disconnected" {
                    debug!("WebRTC disconnected from peer {}: {}", peer_id, state);
                    // Mark the peer as disconnected
                    let _ = self.state.update_webrtc_status(peer_id, false);
                }
            }

            NetworkEvent::WebRtcDataChannelOpened { peer_id, label } => {
                debug!("WebRTC data channel opened for peer {}: {}", peer_id, label);

                // Mark the peer as having data channel connectivity
                let _ = self.state.update_webrtc_status(peer_id, true);
            }

            NetworkEvent::WebRtcDataChannelClosed { peer_id, label } => {
                debug!("WebRTC data channel closed for peer {}: {}", peer_id, label);

                // Mark the peer as having lost data channel connectivity
                let _ = self.state.update_webrtc_status(peer_id, false);
            }

            NetworkEvent::WebRtcDataChannelMessageReceived {
                peer_id,
                label,
                data,
            } => {
                debug!(
                    "WebRTC data channel message received from peer {}: {} bytes",
                    peer_id,
                    data.len()
                );

                // Process data channel messages - could be separate protocol for audio control, chat, etc.
                // For now just log it
            }

            NetworkEvent::WebRtcTrackAdded { peer_id, track_id } => {
                debug!("WebRTC track added for peer {}: {}", peer_id, track_id);

                // In future, we would route this to audio processing
            }

            // Handle the new track received event
            NetworkEvent::WebRtcTrackReceived {
                peer_id,
                track_id,
                kind,
            } => {
                debug!(
                    "WebRTC track received for peer {}: id={}, kind={}",
                    peer_id, track_id, kind
                );
                // TODO: Handle received track (e.g., route to audio pipeline)
            }
        }

        Ok(())
    }

    /// Emit a room event to the UI
    async fn emit_event(&self, event: RoomEvent) -> Result<(), Error> {
        self.event_tx
            .send(event)
            .await
            .map_err(|e| Error::Room(format!("Failed to send room event: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::{self, Receiver, Sender};

    // Helper to set up channels for testing
    fn setup_channels() -> (
        Sender<RoomCommand>,
        Receiver<RoomCommand>,
        Sender<NetworkCommand>,
        Receiver<NetworkCommand>,
        Sender<NetworkEvent>,
        Receiver<NetworkEvent>,
        Sender<RoomEvent>,
        Receiver<RoomEvent>,
    ) {
        let (room_cmd_tx, room_cmd_rx) = mpsc::channel(10);
        let (network_cmd_tx, network_cmd_rx) = mpsc::channel(10);
        let (network_event_tx, network_event_rx) = mpsc::channel(10);
        let (room_event_tx, room_event_rx) = mpsc::channel(10);

        (
            room_cmd_tx,
            room_cmd_rx,
            network_cmd_tx,
            network_cmd_rx,
            network_event_tx,
            network_event_rx,
            room_event_tx,
            room_event_rx,
        )
    }

    #[tokio::test]
    async fn test_create_room() {
        let (
            room_cmd_tx,
            room_cmd_rx,
            network_cmd_tx,
            mut network_cmd_rx,
            _network_event_tx,
            network_event_rx,
            _room_event_tx,
            mut room_event_rx,
        ) = setup_channels();

        // Create handler
        let peer_id = PeerId::new();
        let mut handler = RoomHandler::new(
            peer_id,
            room_cmd_rx,
            network_cmd_tx,
            network_event_rx,
            _room_event_tx,
        );

        // Start handler task
        let handler_task = tokio::spawn(async move {
            handler.run().await.unwrap();
        });

        // Send create room command
        room_cmd_tx.send(RoomCommand::CreateRoom).await.unwrap();

        // Check that NetworkCommand::CreateRoom was sent
        if let Some(NetworkCommand::CreateRoom { room_id }) = network_cmd_rx.recv().await {
            assert!(room_id != RoomId::default());
        } else {
            panic!("Expected NetworkCommand::CreateRoom");
        }

        // Check for PeerAdded event
        if let Some(RoomEvent::PeerAdded(id)) = room_event_rx.recv().await {
            assert_eq!(id, peer_id);
        } else {
            panic!("Expected RoomEvent::PeerAdded");
        }

        // Check for PeerListUpdated event
        if let Some(RoomEvent::PeerListUpdated) = room_event_rx.recv().await {
            // Success
        } else {
            panic!("Expected RoomEvent::PeerListUpdated");
        }

        // Send shutdown command to terminate the handler cleanly
        room_cmd_tx.send(RoomCommand::Shutdown).await.unwrap();

        // Clean up
        handler_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_join_room() {
        let (
            room_cmd_tx,
            room_cmd_rx,
            network_cmd_tx,
            mut network_cmd_rx,
            _network_event_tx,
            network_event_rx,
            _room_event_tx,
            mut room_event_rx,
        ) = setup_channels();

        // Create handler
        let peer_id = PeerId::new();
        let mut handler = RoomHandler::new(
            peer_id,
            room_cmd_rx,
            network_cmd_tx,
            network_event_rx,
            _room_event_tx,
        );

        // Start handler task
        let handler_task = tokio::spawn(async move {
            handler.run().await.unwrap();
        });

        // Create a room ID and address
        let room_id = RoomId::new();
        let address: SocketAddr = "127.0.0.1:12345".parse().unwrap();

        // Send join room command
        room_cmd_tx
            .send(RoomCommand::JoinRoom { room_id, address })
            .await
            .unwrap();

        // Check that NetworkCommand::ConnectToRoom was sent
        if let Some(NetworkCommand::ConnectToRoom {
            room_id: cmd_room_id,
            address: cmd_address,
        }) = network_cmd_rx.recv().await
        {
            assert_eq!(cmd_room_id, room_id);
            assert_eq!(cmd_address, address);
        } else {
            panic!("Expected NetworkCommand::ConnectToRoom");
        }

        // Check for PeerAdded event
        if let Some(RoomEvent::PeerAdded(id)) = room_event_rx.recv().await {
            assert_eq!(id, peer_id);
        } else {
            panic!("Expected RoomEvent::PeerAdded");
        }

        // Check for PeerListUpdated event
        if let Some(RoomEvent::PeerListUpdated) = room_event_rx.recv().await {
            // Success
        } else {
            panic!("Expected RoomEvent::PeerListUpdated");
        }

        // Send shutdown command to terminate the handler cleanly
        room_cmd_tx.send(RoomCommand::Shutdown).await.unwrap();

        // Clean up
        handler_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_handle_join_request() {
        let (
            room_cmd_tx,
            room_cmd_rx,
            network_cmd_tx,
            _network_cmd_rx,
            network_event_tx,
            network_event_rx,
            _room_event_tx,
            mut room_event_rx,
        ) = setup_channels();

        // Create handler
        let peer_id = PeerId::new();
        let mut handler = RoomHandler::new(
            peer_id,
            room_cmd_rx,
            network_cmd_tx,
            network_event_rx,
            _room_event_tx,
        );

        // Start handler task
        let handler_task = tokio::spawn(async move {
            handler.run().await.unwrap();
        });

        // Create a peer ID for the joiner
        let joiner_id = PeerId::new();
        let joiner_name = "Test Joiner".to_string();
        let joiner_addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();

        // Send join requested event
        network_event_tx
            .send(NetworkEvent::JoinRequested {
                peer_id: joiner_id,
                name: joiner_name,
                address: joiner_addr,
            })
            .await
            .unwrap();

        // Check for JoinRequestReceived event
        if let Some(RoomEvent::JoinRequestReceived(id)) = room_event_rx.recv().await {
            assert_eq!(id, joiner_id);
        } else {
            panic!("Expected RoomEvent::JoinRequestReceived");
        }

        // Send shutdown command to terminate the handler cleanly
        room_cmd_tx.send(RoomCommand::Shutdown).await.unwrap();

        // Clean up
        handler_task.await.unwrap();
    }
}
