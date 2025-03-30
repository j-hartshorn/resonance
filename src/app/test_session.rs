use crate::app::session::{Session, SessionError};
use crate::ui::Participant;
use anyhow::{anyhow, Result};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::get_probe;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Manages a test session with simulated participants
pub struct TestSessionManager {
    /// Current test session, if any
    current_session: Option<Session>,
    /// Task handles for audio playback
    playback_tasks: Vec<JoinHandle<()>>,
    /// Path to test audio files
    test_audio_path: PathBuf,
    /// Whether the test is currently running
    active: bool,
    /// Audio data channels for test participants
    participant_audio: Arc<Mutex<Vec<Vec<f32>>>>,
}

impl TestSessionManager {
    /// Creates a new test session manager
    pub fn new() -> Self {
        Self {
            current_session: None,
            playback_tasks: Vec::new(),
            test_audio_path: PathBuf::from("test_audio"),
            active: false,
            participant_audio: Arc::new(Mutex::new(vec![Vec::new(); 3])),
        }
    }

    /// Creates a test session with simulated participants
    pub async fn create_test_session(&mut self) -> Result<Session, SessionError> {
        // First leave any existing session
        if self.current_session.is_some() {
            self.leave_test_session().await?;
        }

        // Generate a session ID
        let session_id = uuid::Uuid::new_v4().to_string();

        // Create participants
        let current_user = Participant::new("Me").with_position(0.0, 0.0, 0.0);
        let participant1 = Participant::new("TestUser1").with_position(-1.0, 0.0, -1.0);
        let participant2 = Participant::new("TestUser2").with_position(0.0, 0.0, -1.5);
        let participant3 = Participant::new("TestUser3").with_position(1.0, 0.0, -1.0);

        // Create session
        let session = Session {
            id: session_id.clone(),
            connection_link: "test-session".to_string(),
            participants: vec![current_user, participant1, participant2, participant3],
            is_host: true,
            original_host_id: "test-host".to_string(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        // Start test audio playback
        self.active = true;
        self.start_test_audio_playback().await?;

        self.current_session = Some(session.clone());
        Ok(session)
    }

    /// Leaves the current test session
    pub async fn leave_test_session(&mut self) -> Result<(), SessionError> {
        if self.current_session.is_some() {
            // Stop all playback tasks
            for task in self.playback_tasks.drain(..) {
                task.abort();
            }

            // Clear the session
            self.current_session = None;
            self.active = false;

            Ok(())
        } else {
            Err(SessionError::NoActiveSession)
        }
    }

    /// Gets the current test session, if any
    pub fn current_session(&self) -> Option<Session> {
        self.current_session.clone()
    }

    /// Starts the playback of test audio files
    async fn start_test_audio_playback(&mut self) -> Result<()> {
        // Clear any existing tasks
        for task in self.playback_tasks.drain(..) {
            task.abort();
        }

        // Validate that test audio files exist
        let test_files = [
            self.test_audio_path.join("test_speech_01.mp3"),
            self.test_audio_path.join("test_speech_02.mp3"),
            self.test_audio_path.join("test_speech_03.mp3"),
            self.test_audio_path.join("test_speech_01_interruption.mp3"),
        ];

        for file in &test_files {
            if !file.exists() {
                return Err(anyhow!("Test audio file not found: {}", file.display()));
            }
        }

        // Create channels for pushing audio data
        let (tx1, rx1) = mpsc::channel::<Vec<f32>>(100);
        let (tx2, rx2) = mpsc::channel::<Vec<f32>>(100);
        let (tx3, rx3) = mpsc::channel::<Vec<f32>>(100);

        // Clone data structures for tasks
        let participant_audio = Arc::clone(&self.participant_audio);

        // Start playback coordinator task
        let handle = tokio::spawn(async move {
            let mut round = 0;

            // Run the test scenario in a loop
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;

                // Play participant 1
                load_and_play_mp3(&test_files[0], tx1.clone()).await;
                tokio::time::sleep(Duration::from_secs(1)).await;

                // Play participant 2
                load_and_play_mp3(&test_files[1], tx2.clone()).await;
                tokio::time::sleep(Duration::from_secs(1)).await;

                // On second and subsequent rounds, interrupt with participant 1
                if round >= 1 {
                    // Start the interruption in a separate task
                    let tx1_interrupt = tx1.clone();
                    let interrupt_file = test_files[3].clone();
                    tokio::spawn(async move {
                        load_and_play_mp3(&interrupt_file, tx1_interrupt).await;
                    });
                }

                // Play participant 3
                load_and_play_mp3(&test_files[2], tx3.clone()).await;

                // Wait longer between rounds
                tokio::time::sleep(Duration::from_secs(2)).await;

                round += 1;
            }
        });

        self.playback_tasks.push(handle);

        // Start tasks to receive the audio data and update the shared buffer
        self.start_audio_receiver(0, rx1).await;
        self.start_audio_receiver(1, rx2).await;
        self.start_audio_receiver(2, rx3).await;

        Ok(())
    }

    /// Starts a receiver task for test audio
    async fn start_audio_receiver(&mut self, index: usize, mut rx: mpsc::Receiver<Vec<f32>>) {
        let participant_audio = Arc::clone(&self.participant_audio);

        let handle = tokio::spawn(async move {
            while let Some(audio_data) = rx.recv().await {
                let mut audio_guard = participant_audio.lock().unwrap();
                if index < audio_guard.len() {
                    audio_guard[index] = audio_data;
                }
            }
        });

        self.playback_tasks.push(handle);
    }

    /// Gets the current audio data for a test participant
    pub fn get_participant_audio(&self, index: usize) -> Vec<f32> {
        let audio_guard = self.participant_audio.lock().unwrap();
        if index < audio_guard.len() {
            audio_guard[index].clone()
        } else {
            Vec::new()
        }
    }
}

impl Clone for TestSessionManager {
    fn clone(&self) -> Self {
        TestSessionManager {
            current_session: self.current_session.clone(),
            playback_tasks: Vec::new(), // Don't clone task handles
            test_audio_path: self.test_audio_path.clone(),
            active: self.active,
            participant_audio: Arc::clone(&self.participant_audio),
        }
    }
}

/// Loads an MP3 file and sends the audio data through the channel
async fn load_and_play_mp3(path: &Path, tx: mpsc::Sender<Vec<f32>>) {
    // For now, just use the fallback audio while we resolve Symphonia integration
    eprintln!("Using test audio for {}", path.display());
    generate_fallback_audio(tx).await;
}

/// Generates fallback audio in case the MP3 can't be loaded
async fn generate_fallback_audio(tx: mpsc::Sender<Vec<f32>>) {
    // Generate 2 seconds of a 440 Hz sine wave at 48kHz
    let sample_rate = 48000;
    let frequency = 440.0;
    let duration = 2.0; // seconds
    let num_samples = (sample_rate as f32 * duration) as usize;

    let mut audio = Vec::with_capacity(num_samples);
    for i in 0..num_samples {
        let time = i as f32 / sample_rate as f32;
        let sample = (2.0 * std::f32::consts::PI * frequency * time).sin() * 0.5;
        audio.push(sample);
    }

    // Send the audio data
    let _ = tx.send(audio).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_test_session() {
        let mut manager = TestSessionManager::new();
        let result = manager.create_test_session().await;

        assert!(
            result.is_ok(),
            "Failed to create test session: {:?}",
            result.err()
        );
        let session = result.unwrap();

        // Check that we have 4 participants (Me + 3 test users)
        assert_eq!(session.participants.len(), 4);

        // Verify participant names
        assert_eq!(session.participants[0].name, "Me");
        assert_eq!(session.participants[1].name, "TestUser1");
        assert_eq!(session.participants[2].name, "TestUser2");
        assert_eq!(session.participants[3].name, "TestUser3");

        // Clean up
        let _ = manager.leave_test_session().await;
    }
}
