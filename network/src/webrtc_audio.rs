//! WebRTC audio track handling for room.rs
//!
//! This module handles the integration between WebRTC audio tracks and the audio system.

use log::{debug, error, info, trace, warn};
use room_core::{AudioBuffer, Error, NetworkEvent, PeerId, CHANNELS, SAMPLE_RATE};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// WebRTC audio track handler
pub struct WebRtcAudioHandler {
    /// Our peer ID
    peer_id: PeerId,
    /// Channel for sending audio from WebRTC to the audio system
    audio_sender: mpsc::Sender<(PeerId, AudioBuffer)>,
    /// Channel for receiving audio from the audio system to send via WebRTC
    audio_receiver: Option<mpsc::Receiver<(PeerId, AudioBuffer)>>,
    /// Map of peer IDs to their latest audio buffer
    peer_buffers: Arc<Mutex<HashMap<PeerId, AudioBuffer>>>,
}

impl WebRtcAudioHandler {
    /// Create a new WebRTC audio handler
    pub fn new(
        peer_id: PeerId,
        audio_sender: mpsc::Sender<(PeerId, AudioBuffer)>,
        audio_receiver: mpsc::Receiver<(PeerId, AudioBuffer)>,
    ) -> Self {
        Self {
            peer_id,
            audio_sender,
            audio_receiver: Some(audio_receiver),
            peer_buffers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Start the audio handler
    pub async fn start(&mut self) -> Result<(), Error> {
        let peer_id = self.peer_id;
        let audio_receiver = self
            .audio_receiver
            .take()
            .ok_or_else(|| Error::Network("Audio receiver already taken".to_string()))?;
        let audio_sender = self.audio_sender.clone();
        let peer_buffers = self.peer_buffers.clone();

        // Task: Process audio from the audio system and send it to WebRTC
        // For now, we're just passing it through, in a real implementation
        // this would feed into WebRTC properly
        tokio::spawn(async move {
            info!("Starting WebRTC audio processor task");
            let mut audio_receiver = audio_receiver;
            while let Some((target_peer, buffer)) = audio_receiver.recv().await {
                // Store latest buffer for this peer
                {
                    let mut buffers = peer_buffers.lock().unwrap();
                    buffers.insert(target_peer, buffer.clone());
                }

                // In a real implementation, we would send this to WebRTC
                // For now, just pass it back to the audio system for testing
                if target_peer != peer_id {
                    // Avoid sending back to ourselves to prevent echo
                    if let Err(e) = audio_sender.send((peer_id, buffer)).await {
                        error!("Failed to send audio buffer: {}", e);
                    }
                }
            }
            info!("WebRTC audio processor task ended");
        });

        info!("WebRTC audio handler started");
        Ok(())
    }

    /// Process an incoming audio buffer from a peer
    pub async fn process_audio(&self, peer_id: PeerId, buffer: AudioBuffer) -> Result<(), Error> {
        // Store the buffer for this peer
        {
            let mut buffers = self.peer_buffers.lock().unwrap();
            buffers.insert(peer_id, buffer.clone());
        }

        // Forward to audio system
        if let Err(e) = self.audio_sender.send((peer_id, buffer)).await {
            error!("Failed to send audio to audio system: {}", e);
            return Err(Error::Network(format!("Failed to send audio: {}", e)));
        }

        Ok(())
    }

    /// Handle network events
    pub async fn handle_event(&self, event: NetworkEvent) -> Result<(), Error> {
        match event {
            NetworkEvent::WebRtcTrackReceived {
                peer_id,
                track_id,
                kind,
            } => {
                if kind == "audio" {
                    info!(
                        "Received WebRTC audio track from peer {}: {}",
                        peer_id, track_id
                    );
                    // The actual audio data will flow through process_audio
                }
            }
            NetworkEvent::WebRtcAudioReceived { peer_id, buffer } => {
                debug!(
                    "Received audio data from peer {}: {} samples",
                    peer_id,
                    buffer.len()
                );
                // Forward audio to the audio system
                self.process_audio(peer_id, buffer).await?;
            }
            _ => {
                // Ignore other events
            }
        }

        Ok(())
    }
}

/// Register audio media capabilities in a WebRTC setup
pub fn register_audio_codecs() -> Result<(), Error> {
    // This is a placeholder for what would be WebRTC codec setup
    // In the actual implementation, this would register audio codecs
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_audio_handler() {
        // Create channels for testing
        let (tx1, mut rx1) = mpsc::channel(10);
        let (tx2, rx2) = mpsc::channel(10);

        // Create handler
        let mut handler = WebRtcAudioHandler::new(PeerId::new(), tx1, rx2);

        // Start handler
        assert!(handler.start().await.is_ok());

        // Send test buffer
        let peer_id = PeerId::new();
        let buffer = vec![0.1, 0.2, 0.3, 0.4];
        assert!(handler.process_audio(peer_id, buffer.clone()).await.is_ok());

        // Check if buffer was forwarded
        if let Ok(Some((p, b))) =
            tokio::time::timeout(std::time::Duration::from_millis(100), rx1.recv()).await
        {
            assert_eq!(p, peer_id);
            assert_eq!(b, buffer);
        } else {
            panic!("No audio buffer received");
        }
    }
}
