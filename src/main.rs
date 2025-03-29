mod app;
mod audio;
mod network;
mod ui;

use app::App;
use audio::{AudioCapture, AudioStreamManager, SpatialAudioProcessor, VoiceProcessor};
use std::env;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use ui::{qr_code::display_connection_options, run_tui, Participant};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    // Create a shared container for the latest audio data
    let latest_audio_data = Arc::new(Mutex::new(Vec::<f32>::new()));
    let latest_audio_data_clone = latest_audio_data.clone();

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

    // For throttling repeated error messages
    let mut last_error_time = std::time::Instant::now();
    let error_throttle_duration = std::time::Duration::from_secs(5);
    let participants_clone = Arc::clone(&participants);

    // Clone the audio data reference for the audio processing task
    let latest_audio_data_for_task = latest_audio_data.clone();

    // Process audio in the background
    tokio::spawn(async move {
        // Local instances for voice/audio processing
        let voice_processor = Arc::new(Mutex::new(
            VoiceProcessor::new()
                .with_vad_threshold(0.05)
                .with_echo_cancellation(true),
        ));
        let spatial_processor = Arc::new(Mutex::new(SpatialAudioProcessor::new()));

        while let Some(audio_data) = rx.recv().await {
            // Store the raw audio data for visualization
            {
                let mut audio_store = latest_audio_data_for_task.lock().unwrap();
                *audio_store = audio_data.clone();
            }

            // Apply voice processing
            let processed = {
                let voice_processor = voice_processor.lock().unwrap();
                voice_processor.process(audio_data.clone())
            };

            // Check for voice activity
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
        if let Err(e) = app.join_p2p_session(&link).await {
            eprintln!("Failed to join session: {}", e);
        }
    }

    // Create a sharable app instance for the TUI
    let shared_app = Arc::new(Mutex::new(app));

    // Create another shared data for the audio visualization
    let audio_data_for_ui = latest_audio_data.clone();

    // Run the original TUI function that handles keyboard events properly
    if let Err(e) = run_tui_with_audio(shared_app.clone(), audio_data_for_ui).await {
        eprintln!("TUI error: {}", e);
    }

    // Cleanup before exit
    let mut app_for_cleanup = match Arc::try_unwrap(shared_app) {
        Ok(mutex) => mutex.into_inner()?,
        Err(_) => {
            eprintln!("Could not get exclusive ownership of App for cleanup");
            // Create a dummy app for cleanup
            let mut app = App::new();
            app.initialize().await?;
            app
        }
    };

    audio_manager.stop_all_streams().await?;
    capture.stop().await?;
    app_for_cleanup.shutdown().await?;

    Ok(())
}

// Modified version of run_tui that uses shared audio data
async fn run_tui_with_audio(
    app: Arc<Mutex<App>>,
    audio_data: Arc<Mutex<Vec<f32>>>,
) -> io::Result<()> {
    // Initialize terminal
    let mut terminal_ui = ui::terminal_ui::TerminalUI::new();
    terminal_ui.initialize()?;

    // Main event loop
    let tick_rate = Duration::from_millis(33); // ~30 FPS
    let mut last_tick = std::time::Instant::now();

    loop {
        // Check if enough time has passed for a frame update
        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        // Poll for events
        if let Some(event) = terminal_ui.poll_events(timeout)? {
            match event {
                crossterm::event::Event::Key(key_event) => {
                    // Handle Ctrl+C for exit
                    if key_event
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL)
                        && key_event.code == crossterm::event::KeyCode::Char('c')
                    {
                        break;
                    }

                    if let Some(action) = terminal_ui.handle_key_event(key_event.code) {
                        // First handle internal actions like copy
                        if terminal_ui.handle_menu_action(action.clone()) {
                            continue;
                        }

                        let mut app_lock = app.lock().unwrap();
                        match action {
                            ui::MenuAction::Join => {
                                // Prompt user for join link or create new session
                                // For this example, we'll just create a new session
                                if let Ok(session) = app_lock.create_p2p_session().await {
                                    terminal_ui
                                        .set_connection_link(Some(session.connection_link.clone()));

                                    // Update participants
                                    terminal_ui.update_participants(session.participants.clone());
                                }
                            }
                            ui::MenuAction::Leave => {
                                if app_lock.has_active_connection().await {
                                    let _ = app_lock.leave_session().await;
                                    terminal_ui.set_connection_link(None);
                                    terminal_ui.update_participants(vec![]);
                                }
                            }
                            ui::MenuAction::CopyLink => {
                                // Already handled in handle_menu_action
                            }
                            ui::MenuAction::Quit => break,
                        }
                    }
                }
                _ => {}
            }
        }

        // Update UI if it's time for a frame
        if last_tick.elapsed() >= tick_rate {
            // Update app state
            {
                let app_lock = app.lock().unwrap();

                // Update participants if in a session
                if let Some(session) = app_lock.current_session() {
                    terminal_ui.update_participants(session.participants.clone());
                }

                // Get the latest audio data for visualization
                let audio_data_snapshot = {
                    let data = audio_data.lock().unwrap();
                    data.clone()
                };

                // Only update if we have data
                if !audio_data_snapshot.is_empty() {
                    terminal_ui.update_audio_data(&audio_data_snapshot);
                }
            }

            // Render UI
            terminal_ui.render(&app.lock().unwrap())?;

            last_tick = std::time::Instant::now();
        }
    }

    // Shutdown terminal
    terminal_ui.shutdown()?;

    Ok(())
}
