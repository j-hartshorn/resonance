mod audio;
mod network;

use audio::{AudioCapture, AudioDeviceManager, SpatialAudioProcessor, VoiceProcessor};
use network::{SecurityModule, SignalingService, WebRtcManager};
use std::io::{self, Write};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting resonance.rs audio communication app");

    // Stage 2.1: Audio Device Enumeration
    println!("Available audio devices:");
    let devices = AudioDeviceManager::enumerate_devices();
    for (i, device) in devices.iter().enumerate() {
        println!("  {}. {}", i + 1, device);
    }

    // Stage 2.2: Basic Audio Capture
    println!("\nStarting audio capture...");
    let mut capture = AudioCapture::new();

    // Set a callback to handle received audio data
    let (tx, mut rx) = mpsc::channel::<Vec<f32>>(100);
    capture.set_data_callback(move |data| {
        let _ = tx.try_send(data);
    });

    // Start capture
    capture.start().await?;
    println!("Audio capture started. Processing for 3 seconds...");

    // Stage 2.3: Voice Processing
    let voice_processor = VoiceProcessor::new()
        .with_vad_threshold(0.05)
        .with_echo_cancellation(true);

    // Stage 2.4: Spatial Audio Processing
    let mut spatial_processor = SpatialAudioProcessor::new();
    spatial_processor.set_source_position(0.5, 0.0, 0.0); // Example position

    // Stage 3.1: Signaling Service
    println!("\nInitializing signaling service...");
    let mut signaling = SignalingService::new();
    signaling.connect().await?;
    println!("Signaling service connected");

    // Stage 3.2: WebRTC Integration
    println!("Initializing WebRTC...");
    let mut webrtc = WebRtcManager::new();
    webrtc.initialize()?;
    println!("WebRTC initialized");

    // Stage 3.3: Security Module
    println!("Setting up security...");
    let mut security = SecurityModule::new();
    let _key_pair = security.generate_key_pair()?;
    println!("Security keys generated");

    // Basic command processing
    println!("\nEnter command ('/help' for commands, '/quit' to exit):");
    io::stdout().flush()?;

    // Process audio in the background while waiting for commands
    let mut frames_processed = 0;
    let max_frames = 10; // Just process a few frames for demo

    tokio::spawn(async move {
        while let Some(audio_data) = rx.recv().await {
            // Voice processing
            let processed = voice_processor.process(audio_data);

            // Voice activity detection
            let has_voice = voice_processor.detect_voice_activity(&processed);
            println!("Frame {}: Voice detected: {}", frames_processed, has_voice);

            // Spatial audio processing (if voice detected)
            if has_voice {
                let spatial_audio = spatial_processor.process(&processed);
                println!("  Spatial audio output: {} samples", spatial_audio.len());
            }

            frames_processed += 1;
            if frames_processed >= max_frames {
                break;
            }
        }
    });

    // Simple command loop
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

    // Stop capture and cleanup
    capture.stop().await?;
    webrtc.close_all_connections().await?;
    signaling.disconnect().await?;

    println!("Application shutdown complete");
    Ok(())
}
