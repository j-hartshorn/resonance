//! Audio subsystem for room.rs
//!
//! This crate provides high-level audio processing capabilities
//! by coordinating lower-level components like audio_io and spatial.

use audio_io::AudioDevice;
use log::{debug, error, info, trace, warn};
use room_core::{AudioBuffer, Error, NetworkEvent, PeerId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Entry point for the audio subsystem
pub struct AudioSystem {
    /// Audio device for capture and playback
    audio_device: Arc<Mutex<AudioDevice>>,
    /// Channel for sending captured audio to WebRTC
    webrtc_sender: mpsc::Sender<(PeerId, AudioBuffer)>,
    /// Channel for receiving audio from WebRTC
    webrtc_receiver: Option<mpsc::Receiver<(PeerId, AudioBuffer)>>,
    /// Channel for sending mixed audio to playback
    playback_sender: mpsc::Sender<AudioBuffer>,
    /// Buffer size for audio processing
    buffer_size: usize,
    /// Map of peer IDs to their latest audio buffer
    peer_buffers: Arc<Mutex<HashMap<PeerId, AudioBuffer>>>,
}

impl AudioSystem {
    /// Create a new audio system
    pub fn new(
        webrtc_sender: mpsc::Sender<(PeerId, AudioBuffer)>,
        webrtc_receiver: mpsc::Receiver<(PeerId, AudioBuffer)>,
        buffer_size: usize,
    ) -> Result<Self, Error> {
        // Initialize audio device
        let audio_device = Arc::new(Mutex::new(AudioDevice::new()?));

        // Create channel for playback
        let (playback_sender, playback_receiver) = mpsc::channel(32);

        // Start playback
        {
            let mut device = audio_device.lock().unwrap();
            device.start_playback(playback_receiver)?;
        }

        Ok(Self {
            audio_device,
            webrtc_sender,
            webrtc_receiver: Some(webrtc_receiver),
            playback_sender,
            buffer_size,
            peer_buffers: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Create a new audio system with a pre-configured audio device
    pub fn with_audio_device(
        audio_device: Arc<Mutex<AudioDevice>>,
        webrtc_sender: mpsc::Sender<(PeerId, AudioBuffer)>,
        webrtc_receiver: mpsc::Receiver<(PeerId, AudioBuffer)>,
        buffer_size: usize,
    ) -> Result<Self, Error> {
        // Create channel for playback
        let (playback_sender, playback_receiver) = mpsc::channel(32);

        // Start playback using the provided audio device
        {
            let mut device = audio_device.lock().unwrap();
            device.start_playback(playback_receiver)?;
        }

        Ok(Self {
            audio_device,
            webrtc_sender,
            webrtc_receiver: Some(webrtc_receiver),
            playback_sender,
            buffer_size,
            peer_buffers: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Start audio capture and processing
    pub fn start(&mut self, local_peer_id: PeerId) -> Result<(), Error> {
        // Create channel for capture
        let (capture_sender, capture_receiver) = mpsc::channel(32);

        // Start capture
        {
            let mut device = self.audio_device.lock().unwrap();
            device.start_capture(capture_sender)?;
        }

        // Clone channels for background tasks
        let webrtc_sender = self.webrtc_sender.clone();
        let playback_sender = self.playback_sender.clone();
        let peer_buffers = self.peer_buffers.clone();
        let buffer_size = self.buffer_size;

        // Take ownership of webrtc_receiver
        let webrtc_receiver = self
            .webrtc_receiver
            .take()
            .ok_or_else(|| Error::Audio("WebRTC receiver already taken".to_string()))?;

        // Task 1: Capture audio and send to WebRTC
        let local_peer = local_peer_id;
        let mut capture_receiver = capture_receiver;
        tokio::spawn(async move {
            info!("Starting audio capture task");
            while let Some(buffer) = capture_receiver.recv().await {
                // Forward captured audio to WebRTC for all peers (to be handled by network)
                if let Err(e) = webrtc_sender.send((local_peer, buffer)).await {
                    error!("Failed to send audio to WebRTC: {}", e);
                }
            }
            info!("Audio capture task ended");
        });

        // Task 2: Process received audio and mix for playback
        let mut receiver = webrtc_receiver;
        tokio::spawn(async move {
            info!("Starting audio mixing task");
            while let Some((peer_id, buffer)) = receiver.recv().await {
                // Store the latest buffer for this peer
                {
                    let mut buffers = peer_buffers.lock().unwrap();
                    buffers.insert(peer_id, buffer);
                }

                // Get a copy of all buffers for mixing
                let buffers_copy = {
                    let buffers = peer_buffers.lock().unwrap();
                    buffers.clone()
                };

                // Mix audio from all peers
                let mixed_buffer = mix_audio(&buffers_copy, buffer_size);

                // Send mixed audio to playback
                if let Err(e) = playback_sender.send(mixed_buffer).await {
                    error!("Failed to send mixed audio to playback: {}", e);
                }
            }
            info!("Audio mixing task ended");
        });

        info!("Audio system started");
        Ok(())
    }

    /// Stop audio processing
    pub fn stop(&self) {
        let mut device = self.audio_device.lock().unwrap();
        device.stop_capture();
        device.stop_playback();
        info!("Audio system stopped");
    }

    /// Handle WebRTC network events
    pub fn handle_network_event(&self, event: NetworkEvent) {
        match event {
            NetworkEvent::WebRtcTrackReceived {
                peer_id,
                track_id,
                kind,
            } => {
                info!(
                    "Received WebRTC track: peer={}, id={}, kind={}",
                    peer_id, track_id, kind
                );
                // No direct handling needed here - WebRTC track data will flow through webrtc_receiver
            }
            _ => {
                // Ignore other events
            }
        }
    }
}

/// Mix audio from multiple peers
fn mix_audio(peer_buffers: &HashMap<PeerId, AudioBuffer>, buffer_size: usize) -> AudioBuffer {
    // Create an empty mixed buffer of the specified size
    let mut mixed_buffer = vec![0.0; buffer_size];

    if peer_buffers.is_empty() {
        return mixed_buffer;
    }

    // Mix all peer buffers
    for buffer in peer_buffers.values() {
        let len = std::cmp::min(buffer_size, buffer.len());
        for i in 0..len {
            mixed_buffer[i] += buffer[i];
        }
    }

    // Normalize the mixed audio to prevent clipping
    let num_peers = peer_buffers.len() as f32;
    if num_peers > 1.0 {
        for sample in mixed_buffer.iter_mut() {
            *sample /= num_peers;
        }
    }

    mixed_buffer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mix_audio() {
        // Create test buffers
        let mut peer_buffers = HashMap::new();
        peer_buffers.insert(PeerId::new(), vec![0.5, 0.5, 0.5, 0.5]);
        peer_buffers.insert(PeerId::new(), vec![0.3, 0.3, 0.3, 0.3]);

        // Mix audio
        let mixed = mix_audio(&peer_buffers, 4);

        // Check result (0.5 + 0.3) / 2 = 0.4
        assert_eq!(mixed.len(), 4);
        for sample in mixed {
            assert!((sample - 0.4).abs() < 0.001);
        }
    }
}
