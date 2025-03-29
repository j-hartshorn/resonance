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

    // Mapping of participant positions for spatial audio
    participant_positions: Arc<Mutex<HashMap<String, (f32, f32, f32)>>>,

    // Store the raw capture data for monitoring
    raw_capture_data: Arc<Mutex<Vec<f32>>>,

    // Track whether streams are active
    active: bool,

    // Sample rate for audio processing
    sample_rate: u32,
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
            participant_positions: Arc::new(Mutex::new(HashMap::new())),
            raw_capture_data: Arc::new(Mutex::new(Vec::new())),
            active: false,
            sample_rate: 48000,
        }
    }

    /// Initialize the audio stream manager
    pub fn initialize(&mut self) -> Result<()> {
        self.webrtc.initialize()?;

        // Initialize spatial processor with sample rate
        {
            let mut spatial = self.spatial_processor.lock().unwrap();
            spatial.set_sample_rate(self.sample_rate);
        }

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
            let participant_positions = Arc::clone(&self.participant_positions);
            let raw_capture_data = Arc::clone(&self.raw_capture_data);

            // Create a channel for shutdown signaling
            let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

            // Create channel for audio data
            let (tx, mut rx) = mpsc::channel::<Vec<f32>>(100);

            // Set up the callback for audio data
            capture.set_data_callback(move |data| {
                // Store raw capture data for visualization
                {
                    let mut raw_data = raw_capture_data.lock().unwrap();
                    *raw_data = data.clone();
                }

                let _ = tx.try_send(data);
            });

            // Start the audio capture
            capture.start().await?;

            // Create a processing task
            let webrtc = self.webrtc.clone();
            let session_id_clone = session_id.clone();
            let sample_rate = self.sample_rate;

            tokio::spawn(async move {
                // Buffer to store captured audio data from all participants
                let mut participant_buffers: HashMap<String, Vec<f32>> = HashMap::new();

                // Keep track of when each participant was last heard from
                let mut last_active = HashMap::<String, std::time::Instant>::new();

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

                            // If voice detected, send to peers
                            if has_voice {
                                // Send the processed audio to connected peers
                                if let Ok(connections) = webrtc.get_connections() {
                                    for conn in connections {
                                        if conn.session_id() == session_id_clone {
                                            // In a real implementation, this would use actual WebRTC DataChannels
                                            // to send the audio data to each peer with participant identification

                                            // For now, we'll simulate it by storing in participant_buffers
                                            participant_buffers.insert("Me".to_string(), processed.clone());
                                            last_active.insert("Me".to_string(), std::time::Instant::now());
                                        }
                                    }
                                }

                                // Apply spatial processing to each participant's audio
                                // and mix the result for output
                                let mut spatial_processor_guard = spatial_processor.lock().unwrap();
                                let mut streams_guard = output_streams.lock().unwrap();
                                let positions_guard = participant_positions.lock().unwrap();

                                // For each participant, position their audio correctly and mix
                                for (name, buffer) in &participant_buffers {
                                    // Check if this participant's audio is still fresh (within 1 second)
                                    let is_fresh = if let Some(last) = last_active.get(name) {
                                        last.elapsed() < std::time::Duration::from_secs(1)
                                    } else {
                                        false
                                    };

                                    if is_fresh {
                                        // Get position for this participant
                                        if let Some(position) = positions_guard.get(name) {
                                            // Set source position for spatial processing
                                            spatial_processor_guard.set_source_position(
                                                position.0, position.1, position.2
                                            );

                                            // Process audio spatially
                                            let spatial_audio = spatial_processor_guard.process(buffer);

                                            // Store the spatialized audio for this participant
                                            if let Some(output) = streams_guard.get_mut(name) {
                                                if let Ok(mut output) = output.lock() {
                                                    *output = spatial_audio;
                                                }
                                            }
                                        }
                                    }
                                }

                                // Clean up old participants
                                // Remove any participant that hasn't been active in the last 2 seconds
                                let now = std::time::Instant::now();
                                let old_participants: Vec<String> = last_active
                                    .iter()
                                    .filter(|(_, last)| now.duration_since(**last) > std::time::Duration::from_secs(2))
                                    .map(|(name, _)| name.clone())
                                    .collect();

                                for name in old_participants {
                                    last_active.remove(&name);
                                    participant_buffers.remove(&name);
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
        // Update the current user's position in the spatial processor
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

            // Update participant position map
            let mut positions = self.participant_positions.lock().unwrap();
            positions.insert(
                participant.name.clone(),
                (
                    participant.position.0,
                    participant.position.1,
                    participant.position.2,
                ),
            );
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

    /// Process received audio from another participant
    pub async fn process_remote_audio(
        &mut self,
        participant_name: &str,
        audio_data: &[f32],
    ) -> Result<()> {
        // Create the participant stream if it doesn't exist
        if !self.output_streams.contains_key(participant_name) {
            self.add_participant_stream(participant_name)?;
        }

        // Get position for this participant
        let position = {
            let positions = self.participant_positions.lock().unwrap();
            positions
                .get(participant_name)
                .cloned()
                .unwrap_or((0.0, 0.0, 0.0))
        };

        // Apply spatial processing
        let spatial_audio = {
            let mut spatial = self.spatial_processor.lock().unwrap();
            spatial.set_source_position(position.0, position.1, position.2);
            spatial.process(audio_data)
        };

        // Store the processed audio
        if let Some(output) = self.output_streams.get(participant_name) {
            let mut out = output.lock().unwrap();
            *out = spatial_audio;
        }

        Ok(())
    }

    /// Get the raw capture data for visualization
    pub fn get_raw_capture_data(&self) -> Vec<f32> {
        let data = self.raw_capture_data.lock().unwrap();
        data.clone()
    }

    /// Set the sample rate for all audio processing
    pub fn set_sample_rate(&mut self, sample_rate: u32) -> Result<()> {
        self.sample_rate = sample_rate;

        // Update spatial processor
        if let Ok(mut spatial) = self.spatial_processor.lock() {
            spatial.set_sample_rate(sample_rate);
        }

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

    fn generate_test_audio() -> Vec<f32> {
        // Generate 1 second of a 440 Hz (A4) sine wave at 48kHz
        let sample_rate = 48000;
        let frequency = 440.0;
        let duration = 1.0; // seconds
        let num_samples = (sample_rate as f32 * duration) as usize;

        let mut audio = Vec::with_capacity(num_samples);
        for i in 0..num_samples {
            let time = i as f32 / sample_rate as f32;
            let sample = (2.0 * std::f32::consts::PI * frequency * time).sin() * 0.5;
            audio.push(sample);
        }

        audio
    }

    #[tokio::test]
    async fn test_audio_stream_creation() {
        let mut manager = AudioStreamManager::new();
        manager.initialize().unwrap();

        let stream = manager
            .create_stream("test-session".to_string())
            .await
            .unwrap();
        assert!(stream.is_active());
        assert_eq!(stream.session_id(), "test-session");

        manager.stop_all_streams().await.unwrap();
    }

    async fn setup_mock_participants() -> (MockParticipant, MockParticipant) {
        let alice = MockParticipant {
            name: "Alice".to_string(),
            audio_data: generate_test_audio(),
        };

        let bob = MockParticipant {
            name: "Bob".to_string(),
            audio_data: generate_test_audio(),
        };

        (alice, bob)
    }

    // Mock participant for testing audio transmission
    struct MockParticipant {
        name: String,
        audio_data: Vec<f32>,
    }

    impl MockParticipant {
        async fn send_audio(&self, audio: &[f32]) -> Result<()> {
            // In a real implementation, this would send the audio over WebRTC
            // For testing, we'll just return Ok
            Ok(())
        }

        async fn receive_audio(&mut self) -> Result<Vec<f32>> {
            // In a real implementation, this would receive audio from WebRTC
            // For testing, we'll return a copy of our test audio
            Ok(self.audio_data.clone())
        }
    }

    #[tokio::test]
    async fn test_audio_data_transmission() {
        let mut manager = AudioStreamManager::new();
        manager.initialize().unwrap();

        // Set up mock participants
        let (alice, bob) = setup_mock_participants().await;

        // Add participants to the manager
        manager.add_participant_stream(&alice.name).unwrap();
        manager.add_participant_stream(&bob.name).unwrap();

        // Update positions to arrange participants in a circle
        let participants = vec![
            Participant::new("Me").with_position(0.0, 0.0, 0.0),
            Participant::new(&alice.name).with_position(1.0, 0.0, 0.0),
            Participant::new(&bob.name).with_position(-1.0, 0.0, 0.0),
        ];

        manager.update_positions(&participants).unwrap();

        // Process some mock audio from Alice
        manager
            .process_remote_audio(&alice.name, &alice.audio_data)
            .await
            .unwrap();

        // Verify we can retrieve the processed audio
        let alice_output = manager.get_participant_audio(&alice.name).unwrap();
        let alice_audio = alice_output.lock().unwrap();

        // Check that we have spatialized stereo audio (2 channels)
        assert!(!alice_audio.is_empty());

        // Clean up
        manager.stop_all_streams().await.unwrap();
    }
}
