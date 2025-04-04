use crate::protocol::{ApplicationMessage, Phase1Message};
use log::{debug, error, info, trace, warn};
use room_core::{AudioBuffer, Error, NetworkEvent, PeerId};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use webrtc::api::{APIBuilder, API};
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_candidate::{RTCIceCandidate, RTCIceCandidateInit};
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::media::Sample;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::track::track_local::TrackLocal;

/// WebRTC interface for handling WebRTC connections
pub struct WebRtcInterface {
    /// Our peer ID
    peer_id: PeerId,
    /// WebRTC API instance
    api: API,
    /// Mapping of peer IDs to peer connections
    peer_connections: Arc<Mutex<HashMap<PeerId, Arc<RTCPeerConnection>>>>,
    /// Channel for sending phase1 messages (for signaling)
    phase1_sender: mpsc::Sender<(PeerId, Phase1Message)>,
    /// Channel for sending network events
    event_sender: mpsc::Sender<NetworkEvent>,
    /// Default RTCConfiguration used for all peer connections
    rtc_config: RTCConfiguration,
}

impl WebRtcInterface {
    /// Create a new WebRTC interface
    pub fn new(
        peer_id: PeerId,
        phase1_sender: mpsc::Sender<(PeerId, Phase1Message)>,
        event_sender: mpsc::Sender<NetworkEvent>,
        stun_servers: Vec<String>,
    ) -> Self {
        // Configure ICE servers (STUN/TURN)
        let mut ice_servers = vec![];
        for stun_server in stun_servers {
            ice_servers.push(RTCIceServer {
                urls: vec![stun_server],
                ..Default::default()
            });
        }

        let rtc_config = RTCConfiguration {
            ice_servers,
            ..Default::default()
        };

        // Create WebRTC API instance
        let api = APIBuilder::new().build();

        Self {
            peer_id,
            api,
            peer_connections: Arc::new(Mutex::new(HashMap::new())),
            phase1_sender,
            event_sender,
            rtc_config,
        }
    }

    /// Create a new peer connection for the given peer ID
    pub async fn create_peer_connection(
        &self,
        peer_id: PeerId,
    ) -> Result<Arc<RTCPeerConnection>, Error> {
        let peer_connections = self.peer_connections.lock().await;
        if let Some(pc) = peer_connections.get(&peer_id) {
            debug!("Re-using existing peer connection for peer {}", peer_id);
            return Ok(pc.clone());
        }
        drop(peer_connections);

        debug!("Creating new peer connection for peer {}", peer_id);
        let pc = self
            .api
            .new_peer_connection(self.rtc_config.clone())
            .await
            .map_err(|e| Error::Network(format!("Failed to create peer connection: {}", e)))?;

        // Set up connection state change handler
        let event_sender = self.event_sender.clone();
        let peer_id_clone = peer_id;
        pc.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
            let event_sender = event_sender.clone();
            let peer_id = peer_id_clone;
            Box::pin(async move {
                let state_str = format!("{:?}", state);
                debug!(
                    "Peer connection state change for {}: {}",
                    peer_id, state_str
                );
                let _ = event_sender
                    .send(NetworkEvent::WebRtcConnectionStateChanged {
                        peer_id,
                        state: state_str,
                    })
                    .await;
            })
        }));

        // Set up data channel handler
        let event_sender = self.event_sender.clone();
        let peer_id_clone = peer_id;
        pc.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
            let event_sender = event_sender.clone();
            let peer_id = peer_id_clone;
            let dc_clone = dc.clone();
            Box::pin(async move {
                let label = dc.label().to_string();
                debug!("Data channel opened for peer {}: {}", peer_id, label);

                // Handle data channel open
                let event_sender_clone = event_sender.clone();
                let peer_id_clone = peer_id;
                let label_clone = label.clone();
                dc.on_open(Box::new(move || {
                    let event_sender = event_sender_clone.clone();
                    let peer_id = peer_id_clone;
                    let label = label_clone.clone();
                    Box::pin(async move {
                        let _ = event_sender
                            .send(NetworkEvent::WebRtcDataChannelOpened { peer_id, label })
                            .await;
                    })
                }));

                // Handle data channel message
                let event_sender_clone = event_sender.clone();
                let peer_id_clone = peer_id;
                let label_clone = label.clone();
                dc.on_message(Box::new(move |msg: DataChannelMessage| {
                    let event_sender = event_sender_clone.clone();
                    let peer_id = peer_id_clone;
                    let label = label_clone.clone();
                    Box::pin(async move {
                        let _ = event_sender
                            .send(NetworkEvent::WebRtcDataChannelMessageReceived {
                                peer_id,
                                label,
                                data: msg.data.to_vec(),
                            })
                            .await;
                    })
                }));

                // Handle data channel close
                let event_sender_clone = event_sender.clone();
                let peer_id_clone = peer_id;
                let label_clone = label.clone();
                dc.on_close(Box::new(move || {
                    let event_sender = event_sender_clone.clone();
                    let peer_id = peer_id_clone;
                    let label = label_clone.clone();
                    Box::pin(async move {
                        let _ = event_sender
                            .send(NetworkEvent::WebRtcDataChannelClosed { peer_id, label })
                            .await;
                    })
                }));
            })
        }));

        // Set up ICE candidate handler
        let phase1_sender = self.phase1_sender.clone();
        let peer_id_clone = peer_id;
        pc.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let phase1_sender = phase1_sender.clone();
            let peer_id = peer_id_clone;
            Box::pin(async move {
                if let Some(candidate) = candidate {
                    debug!(
                        "ICE candidate for peer {}: {}",
                        peer_id,
                        candidate.to_string()
                    );

                    // Create a simple but adequate string representation of the candidate
                    let candidate_json = format!("{{\"candidate\":\"{}\"}}", candidate.to_string());

                    let app_msg = ApplicationMessage::IceCandidate {
                        candidate: candidate_json,
                    };
                    let phase1_msg = Phase1Message::ApplicationMessage { message: app_msg };
                    let _ = phase1_sender.send((peer_id, phase1_msg)).await;
                }
            })
        }));

        // Set up track handler
        let event_sender = self.event_sender.clone();
        let peer_id_clone = peer_id;

        // Use a separate function to avoid lifetime issues
        setup_track_handler(&pc, event_sender, peer_id_clone).await;

        // Store peer connection
        let pc_arc = Arc::new(pc);
        let mut peer_connections = self.peer_connections.lock().await;
        peer_connections.insert(peer_id, pc_arc.clone());

        Ok(pc_arc)
    }

    /// Create a data channel on the peer connection
    pub async fn create_data_channel(
        &self,
        peer_id: PeerId,
        label: &str,
    ) -> Result<Arc<RTCDataChannel>, Error> {
        let pc = self.get_peer_connection(peer_id).await?;

        let dc = pc
            .create_data_channel(label, None)
            .await
            .map_err(|e| Error::Network(format!("Failed to create data channel: {}", e)))?;

        // Set up event handlers
        let event_sender = self.event_sender.clone();
        let peer_id_clone = peer_id;
        let label = label.to_string();

        // On open
        let event_sender_clone = event_sender.clone();
        let peer_id_clone2 = peer_id_clone;
        let label_clone = label.clone();
        dc.on_open(Box::new(move || {
            let event_sender = event_sender_clone.clone();
            let peer_id = peer_id_clone2;
            let label = label_clone.clone();
            Box::pin(async move {
                debug!("Data channel opened: {}", label);
                let _ = event_sender
                    .send(NetworkEvent::WebRtcDataChannelOpened { peer_id, label })
                    .await;
            })
        }));

        // On message
        let event_sender_clone = event_sender.clone();
        let peer_id_clone2 = peer_id_clone;
        let label_clone = label.clone();
        dc.on_message(Box::new(move |msg: DataChannelMessage| {
            let event_sender = event_sender_clone.clone();
            let peer_id = peer_id_clone2;
            let label = label_clone.clone();
            Box::pin(async move {
                let _ = event_sender
                    .send(NetworkEvent::WebRtcDataChannelMessageReceived {
                        peer_id,
                        label,
                        data: msg.data.to_vec(),
                    })
                    .await;
            })
        }));

        // On close
        let event_sender_clone = event_sender.clone();
        let peer_id_clone2 = peer_id_clone;
        let label_clone = label.clone();
        dc.on_close(Box::new(move || {
            let event_sender = event_sender_clone.clone();
            let peer_id = peer_id_clone2;
            let label = label_clone.clone();
            Box::pin(async move {
                debug!("Data channel closed: {}", label);
                let _ = event_sender
                    .send(NetworkEvent::WebRtcDataChannelClosed { peer_id, label })
                    .await;
            })
        }));

        Ok(dc)
    }

    /// Get a peer connection for the given peer ID
    async fn get_peer_connection(&self, peer_id: PeerId) -> Result<Arc<RTCPeerConnection>, Error> {
        let peer_connections = self.peer_connections.lock().await;

        if let Some(pc) = peer_connections.get(&peer_id) {
            Ok(pc.clone())
        } else {
            Err(Error::Network(format!(
                "No peer connection for peer {}",
                peer_id
            )))
        }
    }

    /// Create an SDP offer for a peer
    pub async fn create_offer(&self, peer_id: PeerId) -> Result<(), Error> {
        let pc = self.get_peer_connection(peer_id).await?;

        // Create offer
        let offer = pc
            .create_offer(None)
            .await
            .map_err(|e| Error::Network(format!("Failed to create offer: {}", e)))?;

        // Set local description
        pc.set_local_description(offer.clone())
            .await
            .map_err(|e| Error::Network(format!("Failed to set local description: {}", e)))?;

        // Send offer through Phase1 channel
        debug!("Created offer for peer {}: {}", peer_id, offer.sdp);

        let app_msg = ApplicationMessage::SdpOffer { offer: offer.sdp };

        let phase1_msg = Phase1Message::ApplicationMessage { message: app_msg };
        self.phase1_sender
            .send((peer_id, phase1_msg))
            .await
            .map_err(|e| Error::Network(format!("Failed to send offer: {}", e)))?;

        Ok(())
    }

    /// Handle a received SDP offer
    pub async fn handle_offer(&self, peer_id: PeerId, offer_sdp: String) -> Result<(), Error> {
        debug!("Handling offer from peer {}", peer_id);

        // Get or create peer connection
        let pc = self.create_peer_connection(peer_id).await?;

        // Create session description
        let offer = RTCSessionDescription::offer(offer_sdp)
            .map_err(|e| Error::Network(format!("Failed to parse offer: {}", e)))?;

        // Set remote description
        pc.set_remote_description(offer)
            .await
            .map_err(|e| Error::Network(format!("Failed to set remote description: {}", e)))?;

        // Create answer
        let answer = pc
            .create_answer(None)
            .await
            .map_err(|e| Error::Network(format!("Failed to create answer: {}", e)))?;

        // Set local description
        pc.set_local_description(answer.clone())
            .await
            .map_err(|e| Error::Network(format!("Failed to set local description: {}", e)))?;

        // Send answer through Phase1 channel
        debug!("Created answer for peer {}: {}", peer_id, answer.sdp);

        let app_msg = ApplicationMessage::SdpAnswer { answer: answer.sdp };

        let phase1_msg = Phase1Message::ApplicationMessage { message: app_msg };
        self.phase1_sender
            .send((peer_id, phase1_msg))
            .await
            .map_err(|e| Error::Network(format!("Failed to send answer: {}", e)))?;

        Ok(())
    }

    /// Handle a received SDP answer
    pub async fn handle_answer(&self, peer_id: PeerId, answer_sdp: String) -> Result<(), Error> {
        debug!("Handling answer from peer {}", peer_id);

        // Get peer connection
        let pc = self.get_peer_connection(peer_id).await?;

        // Create session description
        let answer = RTCSessionDescription::answer(answer_sdp)
            .map_err(|e| Error::Network(format!("Failed to parse answer: {}", e)))?;

        // Set remote description
        pc.set_remote_description(answer)
            .await
            .map_err(|e| Error::Network(format!("Failed to set remote description: {}", e)))?;

        Ok(())
    }

    /// Handle a received ICE candidate
    pub async fn handle_ice_candidate(
        &self,
        peer_id: PeerId,
        candidate_json: String,
    ) -> Result<(), Error> {
        debug!("Handling ICE candidate from peer {}", peer_id);

        // Get peer connection
        let pc = self.get_peer_connection(peer_id).await?;

        // Parse candidate JSON
        let candidate_init: RTCIceCandidateInit = serde_json::from_str(&candidate_json)
            .map_err(|e| Error::Network(format!("Failed to parse ICE candidate: {}", e)))?;

        // Add ICE candidate
        pc.add_ice_candidate(candidate_init)
            .await
            .map_err(|e| Error::Network(format!("Failed to add ICE candidate: {}", e)))?;

        Ok(())
    }

    /// Send a message through a data channel
    pub async fn send_data_channel_message(
        &self,
        peer_id: PeerId,
        label: &str,
        data: &[u8],
    ) -> Result<(), Error> {
        // Get peer connection
        let pc = self.get_peer_connection(peer_id).await?;

        // We need to find the right data channel by label
        // Since there's no direct data_channels() method, we'll need to
        // create and use a data channel if it doesn't exist already

        // Try to create a data channel with that label, which will either
        // give us a new one or return an error (likely if it already exists)
        let result = pc.create_data_channel(label, None).await;

        match result {
            Ok(dc) => {
                // Wait for data channel to open
                tokio::time::timeout(std::time::Duration::from_secs(2), async {
                    // Simple polling for open state
                    for _ in 0..20 {
                        if dc.ready_state()
                            == webrtc::data_channel::data_channel_state::RTCDataChannelState::Open
                        {
                            return Ok(());
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                    Err(Error::Network(
                        "Timeout waiting for data channel to open".to_string(),
                    ))
                })
                .await
                .unwrap_or_else(|_| {
                    Err(Error::Network(
                        "Timeout waiting for data channel to open".to_string(),
                    ))
                })?;

                // Send message
                dc.send(&data.to_vec().into()).await.map_err(|e| {
                    Error::Network(format!("Failed to send data channel message: {}", e))
                })?;

                Ok(())
            }
            Err(e) => {
                // This might indicate the channel already exists, or something else
                // For now, just return the error
                Err(Error::Network(format!(
                    "Could not open data channel '{}': {}",
                    label, e
                )))
            }
        }
    }

    /// Close a peer connection
    pub async fn close_peer_connection(&self, peer_id: PeerId) -> Result<(), Error> {
        let mut peer_connections = self.peer_connections.lock().await;
        if let Some(pc) = peer_connections.remove(&peer_id) {
            debug!("Closing peer connection for peer {}", peer_id);
            pc.close()
                .await
                .map_err(|e| Error::Network(format!("Failed to close peer connection: {}", e)))?;
        } else {
            debug!("No peer connection found for peer {}", peer_id);
        }
        Ok(())
    }

    /// Add a track to a peer connection
    pub async fn add_track(
        &self,
        peer_id: PeerId,
        track: Arc<dyn TrackLocal + Send + Sync>,
    ) -> Result<(), Error> {
        let pc = self.get_peer_connection(peer_id).await?;

        debug!("Adding track to peer connection for peer {}", peer_id);
        pc.add_track(track)
            .await
            .map_err(|e| Error::Network(format!("Failed to add track: {}", e)))?;

        Ok(())
    }

    /// Initiate WebRTC connection with a peer
    pub async fn initiate_webrtc_connection(&self, peer_id: PeerId) -> Result<(), Error> {
        debug!("Initiating WebRTC connection with peer {}", peer_id);

        // Create peer connection
        self.create_peer_connection(peer_id).await?;

        // Create data channel for reliable messaging
        let dc = self.create_data_channel(peer_id, "reliable").await?;
        debug!("Created data channel 'reliable' for peer {}", peer_id);

        // Create WebRTC offer
        self.create_offer(peer_id).await?;

        Ok(())
    }
}

/// Set up track handler on a peer connection
async fn setup_track_handler(
    pc: &RTCPeerConnection,
    event_sender: mpsc::Sender<NetworkEvent>,
    peer_id: PeerId,
) {
    pc.on_track(Box::new(move |track, _receiver, _transceiver| {
        let event_sender = event_sender.clone();
        let peer_id = peer_id;
        Box::pin(async move {
            // The track is not an Option
            let track_id = track.id();
            let kind = track.kind();
            debug!(
                "Track received for peer {}: id={}, kind={:?}",
                peer_id, track_id, kind
            );

            // Forward track event
            let _ = event_sender
                .send(NetworkEvent::WebRtcTrackReceived {
                    peer_id,
                    track_id,
                    kind: format!("{:?}", kind),
                })
                .await;

            // Set up track reading for audio
            if kind.to_string().to_lowercase().contains("audio") {
                let peer_id_track = peer_id;
                let event_sender_track = event_sender.clone();
                let track_clone = track.clone();

                // Start a task to read samples from the track
                tokio::spawn(async move {
                    info!(
                        "Starting to read from audio track for peer {}",
                        peer_id_track
                    );

                    let buffer_limit = 960; // Default Opus frame size (20ms @ 48kHz)

                    loop {
                        // Read a packet from the track
                        match track_clone.read_rtp().await {
                            Ok((rtp_packet, _attributes)) => {
                                // In a real implementation, this would go through proper
                                // Opus decoding, but for simplicity we'll convert directly
                                let payload = rtp_packet.payload.clone();

                                // Just send a simple normalized buffer as placeholder
                                // This isn't proper Opus decoding but enough for testing
                                let buffer: Vec<f32> = (0..buffer_limit)
                                    .map(|i| {
                                        // Generate simple sine wave using payload data as seed
                                        if i < payload.len() {
                                            (payload[i] as f32 / 255.0) * 0.5
                                        } else {
                                            0.0
                                        }
                                    })
                                    .collect();

                                // Send audio buffer event
                                let _ = event_sender_track
                                    .send(NetworkEvent::WebRtcAudioReceived {
                                        peer_id: peer_id_track,
                                        buffer,
                                    })
                                    .await;
                            }
                            Err(err) => {
                                if err.to_string().contains("EOF") {
                                    // Track has ended
                                    info!("Audio track for peer {} ended", peer_id_track);
                                    break;
                                } else {
                                    error!("Error reading from audio track: {}", err);
                                    // Small delay to avoid tight loop on error
                                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                                }
                            }
                        }
                    }
                });
            }
        })
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_create_webrtc_interface() {
        let peer_id = PeerId::new();
        let (phase1_tx, _) = mpsc::channel(100);
        let (event_tx, _) = mpsc::channel(100);

        let stun_servers = vec!["stun:stun.l.google.com:19302".to_string()];

        let webrtc_if = WebRtcInterface::new(peer_id, phase1_tx, event_tx, stun_servers);

        assert_eq!(webrtc_if.peer_id, peer_id);
        assert_eq!(webrtc_if.rtc_config.ice_servers.len(), 1);
        assert_eq!(webrtc_if.rtc_config.ice_servers[0].urls.len(), 1);
        assert_eq!(
            webrtc_if.rtc_config.ice_servers[0].urls[0],
            "stun:stun.l.google.com:19302"
        );
    }
}
