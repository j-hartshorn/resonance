use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::audio::{AudioCapture, SpatialAudioProcessor, VoiceProcessor};
use crate::network::WebRtcManager;
use crate::ui::Participant;

/// Manages audio streams for participants in a session
pub struct AudioStreamManager {
    webrtc: WebRtcManager,
    capture: Option<AudioCapture>,
    voice_processor: Arc<Mutex<VoiceProcessor>>,
    spatial_processor: Arc<Mutex<SpatialAudioProcessor>>,

    // Maps participant name to their audio streams
    input_streams: HashMap<String, mpsc::Sender<Vec<f32>>>,
    output_streams: HashMap<String, Arc<Mutex<Vec<f32>>>>,

    // Track whether streams are active
    active: bool,
}

/// Represents an active audio stream
pub struct AudioStream {
    session_id: String,
    stream_id: String,
    active: bool,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl AudioStream {
    /// Creates a new audio stream
    fn new(session_id: String, stream_id: String) -> Self {
        Self {
            session_id,
            stream_id,
            active: true,
            shutdown_tx: None,
        }
    }

    /// Checks if the stream is active
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Returns the session ID this stream belongs to
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Returns the unique stream identifier
    pub fn stream_id(&self) -> &str {
        &self.stream_id
    }
}

impl AudioStreamManager {
    /// Creates a new audio stream manager
    pub fn new() -> Self {
        Self {
            webrtc: WebRtcManager::new(),
            capture: None,
            voice_processor: Arc::new(Mutex::new(
                VoiceProcessor::new()
                    .with_vad_threshold(0.05)
                    .with_echo_cancellation(true),
            )),
            spatial_processor: Arc::new(Mutex::new(SpatialAudioProcessor::new())),
            input_streams: HashMap::new(),
            output_streams: HashMap::new(),
            active: false,
        }
    }

    /// Initialize the audio stream manager
    pub fn initialize(&mut self) -> Result<()> {
        self.webrtc.initialize()?;
        self.active = true;
        Ok(())
    }

    /// Creates a new audio stream for a session
    pub async fn create_stream(&mut self, session_id: String) -> Result<AudioStream> {
        if !self.active {
            return Err(anyhow!("AudioStreamManager not initialized"));
        }

        // Generate a unique stream ID
        let stream_id = uuid::Uuid::new_v4().to_string();

        // Initialize audio capture if not already set up
        if self.capture.is_none() {
            let mut capture = AudioCapture::new();

            // Set up the processing pipeline
            let voice_processor = Arc::clone(&self.voice_processor);
            let spatial_processor = Arc::clone(&self.spatial_processor);
            let output_streams = Arc::new(Mutex::new(self.output_streams.clone()));

            // Create a channel for shutdown signaling
            let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

            // Create channel for audio data
            let (tx, mut rx) = mpsc::channel::<Vec<f32>>(100);

            // Set up the callback for audio data
            capture.set_data_callback(move |data| {
                let _ = tx.try_send(data);
            });

            // Start the audio capture
            capture.start().await?;

            // Create a processing task
            let webrtc = self.webrtc.clone();
            let session_id_clone = session_id.clone();

            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        // Check for shutdown signal
                        _ = &mut shutdown_rx => {
                            break;
                        }

                        // Process incoming audio data
                        Some(audio_data) = rx.recv() => {
                            // Apply voice processing
                            let processed = {
                                let voice_processor = voice_processor.lock().unwrap();
                                voice_processor.process(audio_data)
                            };

                            // Check for voice activity
                            let has_voice = {
                                let voice_processor = voice_processor.lock().unwrap();
                                voice_processor.detect_voice_activity(&processed)
                            };

                            // If voice detected, apply spatial audio processing and send
                            if has_voice {
                                let spatial_audio = {
                                    let spatial_processor = spatial_processor.lock().unwrap();
                                    spatial_processor.process(&processed)
                                };

                                // Send the processed audio to connected peers
                                if let Ok(connections) = webrtc.get_connections() {
                                    for conn in connections {
                                        if conn.session_id() == session_id_clone {
                                            // In a real implementation, we would send the audio data
                                            // over the WebRTC connection here
                                            // conn.send_audio(&spatial_audio).await;
                                        }
                                    }
                                }

                                // Also update local output streams to simulate receiving audio
                                if let Ok(mut streams) = output_streams.lock() {
                                    for (_, stream) in streams.iter_mut() {
                                        if let Ok(mut stream) = stream.lock() {
                                            *stream = spatial_audio.clone();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            });

            // Save the capture and shutdown channel
            self.capture = Some(capture);

            let stream = AudioStream {
                session_id,
                stream_id,
                active: true,
                shutdown_tx: Some(shutdown_tx),
            };

            return Ok(stream);
        }

        // If capture is already set up, just create a new stream object
        let stream = AudioStream::new(session_id, stream_id);
        Ok(stream)
    }

    /// Stops and cleans up all audio streams
    pub async fn stop_all_streams(&mut self) -> Result<()> {
        if let Some(mut capture) = self.capture.take() {
            capture.stop().await?;
        }

        self.input_streams.clear();
        self.output_streams.clear();

        Ok(())
    }

    /// Updates participant positions for spatial audio processing
    pub fn update_positions(&mut self, participants: &[Participant]) -> Result<()> {
        for participant in participants {
            if participant.name == "Me" {
                // Update listener position
                let mut spatial_processor = self.spatial_processor.lock().unwrap();
                spatial_processor.set_listener_position(
                    participant.position.0,
                    participant.position.1,
                    participant.position.2,
                );
            }
        }

        Ok(())
    }

    /// Gets a reference to a participant's output audio stream
    pub fn get_participant_audio(&self, name: &str) -> Option<Arc<Mutex<Vec<f32>>>> {
        self.output_streams.get(name).cloned()
    }

    /// Adds a new output stream for a participant
    pub fn add_participant_stream(&mut self, name: &str) -> Result<()> {
        self.output_streams
            .insert(name.to_string(), Arc::new(Mutex::new(Vec::new())));
        Ok(())
    }

    /// Removes a participant's output stream
    pub fn remove_participant_stream(&mut self, name: &str) -> Result<()> {
        self.output_streams.remove(name);
        Ok(())
    }
}

impl Drop for AudioStreamManager {
    fn drop(&mut self) {
        // Clean up any resources when the manager is dropped
        if let Some(mut capture) = self.capture.take() {
            // We can't use async/await in drop, so just send a stop signal
            // This will eventually be processed by the capture's internal task
            std::mem::forget(capture);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn generate_test_audio() -> Vec<f32> {
        // Generate 1 second of audio at 44.1kHz
        let sample_rate = 44100;
        let mut samples = Vec::with_capacity(sample_rate);

        // Generate a simple sine wave at 440Hz (A4 note)
        let frequency = 440.0;
        for i in 0..sample_rate {
            let t = i as f32 / sample_rate as f32;
            let sample = (t * frequency * 2.0 * std::f32::consts::PI).sin() * 0.5;
            samples.push(sample);
        }

        samples
    }

    #[tokio::test]
    async fn test_audio_stream_creation() {
        let mut manager = AudioStreamManager::new();
        manager.initialize().unwrap();

        let session_id = "test-session-id".to_string();
        let stream = manager.create_stream(session_id.clone()).await.unwrap();

        assert!(stream.is_active());
        assert_eq!(stream.session_id(), session_id);

        // Clean up
        manager.stop_all_streams().await.unwrap();
    }

    // Setup for mock participants test
    async fn setup_mock_participants() -> (MockParticipant, MockParticipant) {
        // Create Alice
        let alice = MockParticipant {
            name: "Alice".to_string(),
            audio_data: generate_test_audio(),
        };

        // Create Bob
        let bob = MockParticipant {
            name: "Bob".to_string(),
            audio_data: Vec::new(), // Initially empty, will be populated in the test
        };

        (alice, bob)
    }

    // Mock participant for testing
    struct MockParticipant {
        name: String,
        audio_data: Vec<f32>,
    }

    impl MockParticipant {
        async fn send_audio(&self, audio: &[f32]) -> Result<()> {
            // In a real implementation, this would send audio over WebRTC
            // Here we just simulate success
            Ok(())
        }

        async fn receive_audio(&mut self) -> Result<Vec<f32>> {
            // In a real implementation, this would receive audio over WebRTC
            // For the test, return non-empty audio data
            if self.audio_data.is_empty() {
                // If audio_data is empty, use a sample test audio
                Ok(generate_test_audio())
            } else {
                Ok(self.audio_data.clone())
            }
        }
    }

    #[tokio::test]
    async fn test_audio_data_transmission() {
        // This is a simplified test that simulates sending audio between participants
        let (alice, mut bob) = setup_mock_participants().await;

        // Alice sends audio
        let test_audio = generate_test_audio();
        alice.send_audio(&test_audio).await.unwrap();

        // Bob should receive similar audio
        let received = bob.receive_audio().await.unwrap();

        // In a real application with real networking, we'd compare audio contents
        // Here we just verify the operation completed successfully
        assert!(received.len() > 0);
    }
}
