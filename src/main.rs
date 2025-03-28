mod audio;
mod network;
mod ui;

use audio::{AudioCapture, SpatialAudioProcessor, VoiceProcessor};
use network::{SecurityModule, SignalingService, WebRtcManager};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use ui::Participant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting resonance.rs audio communication app");

    // Initialize components
    let mut signaling = SignalingService::new();
    signaling.connect().await?;

    let mut webrtc = WebRtcManager::new();
    webrtc.initialize()?;

    let security = SecurityModule::new();
    let voice_processor = VoiceProcessor::new()
        .with_vad_threshold(0.05)
        .with_echo_cancellation(true);

    let mut spatial_processor = SpatialAudioProcessor::new();

    let current_user = Participant::new("Me").with_position(0.0, 0.0, 0.0);
    let participants = Arc::new(Mutex::new(vec![current_user.clone()]));

    // Basic Audio Capture
    let mut capture = AudioCapture::new();

    // Set a callback to handle received audio data
    let (tx, mut rx) = mpsc::channel::<Vec<f32>>(100);
    capture.set_data_callback(move |data| {
        let _ = tx.try_send(data);
    });

    // Start capture
    capture.start().await?;

    // Process audio in the background
    let voice_processor = Arc::new(Mutex::new(voice_processor));
    let spatial_processor = Arc::new(Mutex::new(spatial_processor));
    let participants_clone = Arc::clone(&participants);

    tokio::spawn(async move {
        while let Some(audio_data) = rx.recv().await {
            // Voice processing
            let processed = {
                let voice_processor = voice_processor.lock().unwrap();
                voice_processor.process(audio_data)
            };

            // Voice activity detection and update speaking status
            let has_voice = {
                let voice_processor = voice_processor.lock().unwrap();
                voice_processor.detect_voice_activity(&processed)
            };

            // Update participants (including ourselves) with speaking status
            let mut participants = participants_clone.lock().unwrap();
            if !participants.is_empty() {
                participants[0].is_speaking = has_voice;
            }

            // Spatial audio processing (if voice detected)
            if has_voice {
                let spatial_audio = {
                    let spatial_processor = spatial_processor.lock().unwrap();
                    spatial_processor.process(&processed)
                };

                // In a real app, send this spatial audio to other participants
                // via WebRTC here
            }
        }
    });

    // Simple command loop
    println!("\nEnter command ('/help' for commands, '/quit' to exit):");
    io::stdout().flush()?;

    let mut command = String::new();
    loop {
        command.clear();
        io::stdin().read_line(&mut command)?;
        let trimmed = command.trim();

        match trimmed {
            "/help" => {
                println!("Available commands:");
                println!("  /create - Create a new session");
                println!("  /join <link> - Join an existing session");
                println!("  /quit - Exit the application");
            }
            "/create" => {
                let session = signaling.create_session().await?;
                println!("Session created. Share this link to let others join:");
                println!("  {}", session.connection_link);
            }
            cmd if cmd.starts_with("/join ") => {
                let link = &cmd[6..];
                match signaling.join_session(link).await {
                    Ok(_) => println!("Joined session successfully"),
                    Err(e) => println!("Failed to join session: {}", e),
                }
            }
            "/quit" => {
                println!("Shutting down...");
                break;
            }
            _ => {
                if !trimmed.is_empty() {
                    println!("Unknown command. Type '/help' for available commands.");
                }
            }
        }

        io::stdout().flush()?;
    }

    // Cleanup before exit
    capture.stop().await?;
    webrtc.close_all_connections().await?;
    signaling.disconnect().await?;

    println!("Application shutdown complete");
    Ok(())
}
