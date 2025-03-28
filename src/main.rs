mod app;
mod audio;
mod network;
mod ui;

use app::App;
use audio::{AudioCapture, AudioStreamManager, SpatialAudioProcessor, VoiceProcessor};
use std::env;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use ui::{qr_code::display_connection_options, Participant};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting resonance.rs audio communication app");

    // Check if we're joining from a link via command line
    let args: Vec<String> = env::args().collect();
    let mut join_link = None;

    if args.len() > 2 && args[1] == "join" {
        join_link = Some(args[2].clone());
    }

    // Initialize application
    let mut app = App::new();
    app.initialize().await?;

    // Initialize the audio stream manager
    let mut audio_manager = AudioStreamManager::new();
    audio_manager.initialize()?;

    // Basic Audio Capture for testing
    let mut capture = AudioCapture::new();

    // Set a callback to handle received audio data
    let (tx, mut rx) = mpsc::channel::<Vec<f32>>(100);
    capture.set_data_callback(move |data| {
        let _ = tx.try_send(data);
    });

    // Start capture
    capture.start().await?;

    // Create participant for ourselves
    let current_user = Participant::new("Me").with_position(0.0, 0.0, 0.0);
    let participants = Arc::new(Mutex::new(vec![current_user.clone()]));

    // Create a reference to app.session_manager that will be moved to the background task
    let session_manager_ref = app.session_manager.clone();

    // Create a separate reference to app that implements has_active_connection()
    let mut app_check_connection = App::new();
    app_check_connection.initialize().await?;

    // Important - use the same session_manager so we get accurate connection state
    app_check_connection.session_manager = app.session_manager.clone();

    let app_check_connection = Arc::new(app_check_connection);
    let app_check_connection_clone = Arc::clone(&app_check_connection);

    // Process audio in the background
    let voice_processor = Arc::new(Mutex::new(
        VoiceProcessor::new()
            .with_vad_threshold(0.05)
            .with_echo_cancellation(true),
    ));
    let spatial_processor = Arc::new(Mutex::new(SpatialAudioProcessor::new()));
    let participants_clone = Arc::clone(&participants);

    // For throttling repeated error messages
    let mut last_error_time = std::time::Instant::now();
    let error_throttle_duration = std::time::Duration::from_secs(5);

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

            // Update participants speaking status
            {
                let mut participants = participants_clone.lock().unwrap();
                if !participants.is_empty() {
                    participants[0].is_speaking = has_voice;
                }
            }

            // Spatial audio processing (if voice detected)
            if has_voice {
                let _spatial_audio = {
                    let spatial_processor = spatial_processor.lock().unwrap();
                    spatial_processor.process(&processed)
                };

                // First check if we have an active connection
                let has_connection = app_check_connection_clone.has_active_connection().await;

                // Only attempt to send audio if we have an active connection
                if has_connection {
                    // In a real app, send this spatial audio to other participants
                    // via our P2P connections
                    if let Some(session_manager) = &session_manager_ref {
                        if let Err(e) = session_manager.send_audio_data(&processed).await {
                            let now = std::time::Instant::now();
                            if now.duration_since(last_error_time) > error_throttle_duration {
                                eprintln!("Error sending audio: {}", e);
                                last_error_time = now;
                            }
                        }
                    }
                }
            }
        }
    });

    // If we have a join link from command line, try to join immediately
    if let Some(link) = join_link {
        match app.join_p2p_session(&link).await {
            Ok(_) => {
                if let Some(session) = app.current_session() {
                    println!("Joined session successfully: {}", session.id);
                }
            }
            Err(e) => println!("Failed to join session: {}", e),
        }
    }

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
                println!("  /create - Create a new P2P session");
                println!("  /join <link> - Join an existing session");
                println!("  /leave - Leave the current session");
                println!("  /status - Show current session status");
                println!("  /quit - Exit the application");
            }
            "/create" => {
                match app.create_p2p_session().await {
                    Ok(session) => {
                        // Create an audio stream for this session
                        let _stream = audio_manager.create_stream(session.id.clone()).await?;

                        // Display QR code and other sharing options
                        display_connection_options(&session.connection_link)?;
                    }
                    Err(e) => println!("Failed to create session: {}", e),
                }
            }
            cmd if cmd.starts_with("/join ") => {
                let link = &cmd[6..];
                match app.join_p2p_session(link).await {
                    Ok(_) => {
                        if let Some(session) = app.current_session() {
                            // Create an audio stream for this session
                            let _stream = audio_manager.create_stream(session.id.clone()).await?;

                            println!("Joined session successfully");

                            // Add streams for other participants
                            for participant in &session.participants {
                                if participant.name != "Me" {
                                    audio_manager.add_participant_stream(&participant.name)?;
                                }
                            }
                        }
                    }
                    Err(e) => println!("Failed to join session: {}", e),
                }
            }
            "/leave" => {
                match app.leave_session().await {
                    Ok(_) => {
                        // Stop audio streams
                        audio_manager.stop_all_streams().await?;
                        println!("Left session successfully");
                    }
                    Err(e) => println!("{}", e),
                }
            }
            "/status" => {
                if let Some(session) = app.current_session() {
                    println!("Current session: {}", session.id);
                    println!(
                        "  Host: {}",
                        if session.is_host {
                            "You"
                        } else {
                            "Someone else"
                        }
                    );
                    println!("  Participants:");

                    for participant in &session.participants {
                        println!(
                            "    - {} {}",
                            participant.name,
                            if participant.is_speaking {
                                "(speaking)"
                            } else {
                                ""
                            }
                        );
                    }

                    // Show connection state if available
                    if let Some(conn_state) = app.connection_state().await {
                        println!("  Connection: {:?}", conn_state);
                    }
                } else {
                    println!("Not in a session");
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
    audio_manager.stop_all_streams().await?;
    capture.stop().await?;
    app.shutdown().await?;

    println!("Application shutdown complete");
    Ok(())
}
