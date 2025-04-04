//! Network module for room.rs
//!
//! This module handles all network communication for the application,
//! from basic UDP sockets to WebRTC connection management.

use log::{debug, error, info, trace, warn};
use room_core::{AudioBuffer, Error, NetworkCommand, NetworkEvent, NetworkMessage, PeerId, RoomId};
use settings_manager::ConfigManager;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

pub mod events;
pub mod phase1;
pub mod protocol;
pub mod stun_client;
pub mod webrtc_audio;
pub mod webrtc_if;

use crate::phase1::Phase1Network;
use crate::protocol::Phase1Message;
use crate::webrtc_audio::WebRtcAudioHandler;
use crate::webrtc_if::WebRtcInterface;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_network_manager() {
        let peer_id = PeerId::new();
        let manager = NetworkManager::new(peer_id).await;
        assert!(manager.is_ok());

        let manager = manager.unwrap();
        assert_eq!(manager.room_id, None);
    }

    mod webrtc_tests;
}

/// Network manager coordinates all networking operations
pub struct NetworkManager {
    /// Our peer ID
    peer_id: PeerId,
    /// Current room ID
    room_id: Option<RoomId>,
    /// Phase 1 network (UDP-based secure channel)
    phase1: Phase1Network,
    /// WebRTC interface for managing WebRTC connections
    webrtc: WebRtcInterface,
    /// WebRTC audio handler (optional)
    webrtc_audio: Option<WebRtcAudioHandler>,
    /// Channel for sending network events
    event_tx: mpsc::Sender<NetworkEvent>,
    /// Channel for receiving network events (for internal forwarding)
    event_rx: mpsc::Receiver<NetworkEvent>,
    /// Channel for receiving commands from room
    command_rx: mpsc::Receiver<NetworkCommand>,
    /// Channel for forwarding messages from WebRTC to Phase1
    phase1_tx: mpsc::Sender<(PeerId, protocol::Phase1Message)>,
    /// Channel for receiving messages from Phase1 to WebRTC
    phase1_rx: mpsc::Receiver<(PeerId, protocol::Phase1Message)>,
    /// Channel for sending audio to/from the audio system
    audio_tx: Option<mpsc::Sender<(PeerId, AudioBuffer)>>,
    /// Channel for receiving audio from the audio system
    audio_rx: Option<mpsc::Receiver<(PeerId, AudioBuffer)>>,
}

impl NetworkManager {
    /// Create a new network manager
    pub async fn new(peer_id: PeerId) -> Result<Self, Error> {
        // Get settings for STUN servers
        let config_manager = ConfigManager::new()?;
        let settings = config_manager.settings();

        // Create channels
        let (event_tx, event_rx) = mpsc::channel(100);
        let (phase1_tx, phase1_rx) = mpsc::channel(100);
        let (command_tx, command_rx) = mpsc::channel(100);

        // Create Phase 1 network with default bind address
        let bind_addr = SocketAddr::new(
            std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)),
            0, // Use port 0 to get a random available port
        );
        let phase1 = Phase1Network::new(peer_id, Some(bind_addr), event_tx.clone()).await?;

        // Create WebRTC interface with STUN servers from settings
        let webrtc = WebRtcInterface::new(
            peer_id,
            phase1_tx.clone(),
            event_tx.clone(),
            settings.ice_servers.clone(),
        );

        Ok(Self {
            peer_id,
            room_id: None,
            phase1,
            webrtc,
            webrtc_audio: None,
            event_tx,
            event_rx,
            command_rx,
            phase1_tx,
            phase1_rx,
            audio_tx: None,
            audio_rx: None,
        })
    }

    /// Set up audio channels for WebRTC audio
    pub fn set_audio_channels(
        &mut self,
        to_audio_tx: mpsc::Sender<(PeerId, AudioBuffer)>,
        from_audio_rx: mpsc::Receiver<(PeerId, AudioBuffer)>,
    ) {
        self.audio_tx = Some(to_audio_tx);
        self.audio_rx = Some(from_audio_rx);
    }

    /// Initialize WebRTC audio handler
    pub async fn init_audio(&mut self) -> Result<(), Error> {
        // If audio channels are set, create the audio handler
        if let (Some(audio_tx), Some(audio_rx)) = (self.audio_tx.take(), self.audio_rx.take()) {
            // Create audio handler
            let webrtc_audio = WebRtcAudioHandler::new(self.peer_id, audio_tx, audio_rx);

            // Store the audio handler
            self.webrtc_audio = Some(webrtc_audio);

            // Start the audio handler
            if let Some(audio_handler) = &mut self.webrtc_audio {
                audio_handler.start().await?;
                info!("WebRTC audio handler initialized");
            }
        } else {
            debug!("Audio channels not set, skipping audio initialization");
        }

        Ok(())
    }

    /// Get a sender for commands to this network manager
    pub fn get_command_sender(&self) -> mpsc::Sender<NetworkCommand> {
        mpsc::Sender::clone(&self.phase1.get_command_sender())
    }

    /// Start the network manager
    pub async fn run(&mut self) -> Result<(), Error> {
        // Start the Phase1Network
        self.phase1.start().await?;

        // Process commands
        loop {
            tokio::select! {
                // Process commands
                Some(command) = self.command_rx.recv() => {
                    if let Err(e) = self.handle_command(command).await {
                        error!("Error handling network command: {}", e);
                    }
                }

                // Process Phase1 messages for WebRTC signaling
                Some((peer_id, message)) = self.phase1_rx.recv() => {
                    if let Err(e) = self.handle_phase1_message(peer_id, message).await {
                        error!("Error handling Phase1 message for WebRTC: {}", e);
                    }
                }

                // Check for task cancellation
                else => break,
            }
        }

        Ok(())
    }

    /// Handle a command from room
    async fn handle_command(&mut self, command: NetworkCommand) -> Result<(), Error> {
        match command {
            NetworkCommand::CreateRoom { room_id } => {
                self.create_room(room_id).await?;
            }

            NetworkCommand::ConnectToRoom { room_id, address } => {
                self.connect_to_room(room_id, address).await?;
            }

            NetworkCommand::SendJoinResponse {
                peer_id,
                approved,
                reason,
            } => {
                self.send_join_response(peer_id, approved, reason).await?;

                // If approved, establish WebRTC connection
                if approved {
                    debug!("Join approved for peer {}, initiating WebRTC", peer_id);
                    self.initiate_webrtc_connection(peer_id).await?;
                }
            }

            NetworkCommand::InitiateWebRtcConnection { peer_id } => {
                self.initiate_webrtc_connection(peer_id).await?;
            }

            NetworkCommand::HandleWebRtcOffer { peer_id, offer } => {
                self.webrtc.handle_offer(peer_id, offer).await?;
            }

            NetworkCommand::HandleWebRtcAnswer { peer_id, answer } => {
                self.webrtc.handle_answer(peer_id, answer).await?;
            }

            NetworkCommand::HandleWebRtcIceCandidate { peer_id, candidate } => {
                self.webrtc.handle_ice_candidate(peer_id, candidate).await?;
            }

            NetworkCommand::SendWebRtcDataChannelMessage {
                peer_id,
                label,
                data,
            } => {
                self.webrtc
                    .send_data_channel_message(peer_id, &label, &data)
                    .await?;
            }

            NetworkCommand::DisconnectPeer { peer_id } => {
                self.disconnect_peer(peer_id).await?;
            }
        }

        Ok(())
    }

    /// Handle a network event for audio
    async fn handle_audio_event(&mut self, event: NetworkEvent) -> Result<(), Error> {
        if let Some(audio_handler) = &self.webrtc_audio {
            audio_handler.handle_event(event.clone()).await?;
        }
        Ok(())
    }

    /// Handle a Phase1 message for WebRTC signaling
    async fn handle_phase1_message(
        &self,
        peer_id: PeerId,
        message: protocol::Phase1Message,
    ) -> Result<(), Error> {
        // Process ApplicationMessage variant
        if let protocol::Phase1Message::ApplicationMessage { message: app_msg } = message {
            match app_msg {
                protocol::ApplicationMessage::SdpOffer { offer } => {
                    debug!("Received SDP offer from peer {}", peer_id);
                    self.webrtc.handle_offer(peer_id, offer).await?;
                }

                protocol::ApplicationMessage::SdpAnswer { answer } => {
                    debug!("Received SDP answer from peer {}", peer_id);
                    self.webrtc.handle_answer(peer_id, answer).await?;
                }

                protocol::ApplicationMessage::IceCandidate { candidate } => {
                    debug!("Received ICE candidate from peer {}", peer_id);
                    self.webrtc.handle_ice_candidate(peer_id, candidate).await?;
                }

                _ => {
                    // Other application messages not relevant to WebRTC
                    debug!(
                        "Received non-WebRTC application message from peer {}",
                        peer_id
                    );
                }
            }
        }

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

    /// Initiate WebRTC connection with a peer
    pub async fn initiate_webrtc_connection(&self, peer_id: PeerId) -> Result<(), Error> {
        debug!("Initiating WebRTC connection with peer {}", peer_id);

        // Use the WebRTC interface to handle connection setup
        self.webrtc.initiate_webrtc_connection(peer_id).await?;

        // Log whether audio is enabled
        if self.audio_tx.is_some() {
            info!("Audio channels are set up for peer {}", peer_id);
        } else {
            warn!("Audio channels are NOT set up for peer {}", peer_id);
        }

        Ok(())
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
        // Close WebRTC connection
        let _ = self.webrtc.close_peer_connection(peer_id).await;

        // Disconnect via Phase1Network
        self.phase1.disconnect_peer(peer_id).await
    }
}
