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
use ui::{qr_code::display_connection_options, run_tui, Participant, SettingsManager};

// Default sample rate for all audio processing
const DEFAULT_SAMPLE_RATE: u32 = 48000;

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

    // Create and initialize audio manager
    let mut audio_manager = AudioStreamManager::new();
    audio_manager.set_sample_rate(DEFAULT_SAMPLE_RATE)?;
    audio_manager.initialize()?;

    // Load device settings from application config
    let app_config = app.config().clone();

    // Set input device if configured
    if let Some(input_device_id) = &app_config.input_device {
        // Find the device with the matching ID
        let input_devices = audio_manager.get_input_devices();
        if let Some(device) = input_devices.iter().find(|d| &d.id == input_device_id) {
            match audio_manager.set_input_device(device.clone()).await {
                Ok(_) => println!("Using configured input device: {}", device.name),
                Err(e) => eprintln!("Failed to set input device from config: {}", e),
            }
        }
    }

    // Set output device if configured
    if let Some(output_device_id) = &app_config.output_device {
        // Find the device with the matching ID
        let output_devices = audio_manager.get_output_devices();
        if let Some(device) = output_devices.iter().find(|d| &d.id == output_device_id) {
            match audio_manager.set_output_device(device.clone()) {
                Ok(_) => println!("Using configured output device: {}", device.name),
                Err(e) => eprintln!("Failed to set output device from config: {}", e),
            }
        }
    }

    // Create a default session stream
    match audio_manager
        .create_stream("default-session".to_string())
        .await
    {
        Ok(_stream) => {
            // Successfully created stream
        }
        Err(e) => {
            eprintln!("Error creating audio stream: {}", e);
        }
    }

    // Create participant for ourselves with initial position at the center (0,0,0)
    let current_user = Participant::new("Me").with_position(0.0, 0.0, 0.0);
    let participants = Arc::new(Mutex::new(vec![current_user.clone()]));

    // Update audio manager with initial participant positions
    {
        let participants_guard = participants.lock().unwrap();
        audio_manager.update_positions(&participants_guard)?;
    }

    // Create a reference to app.session_manager that will be moved to the background task
    let session_manager_ref = app.session_manager.clone();

    // Create a separate reference to app that implements has_active_connection()
    let mut app_check_connection = App::new();
    app_check_connection.initialize().await?;

    // Important - use the same session_manager so we get accurate connection state
    app_check_connection.session_manager = app.session_manager.clone();

    let app_check_connection = Arc::new(app_check_connection);

    // Create shared audio manager for TUI
    let audio_manager = Arc::new(Mutex::new(audio_manager));
    let participants_clone = Arc::clone(&participants);

    // If we have a join link from command line, try to join immediately
    if let Some(link) = join_link {
        if let Err(e) = app.join_p2p_session(&link).await {
            eprintln!("Failed to join session: {}", e);
        }
    }

    // Create a sharable app instance for the TUI
    let shared_app = Arc::new(Mutex::new(app));

    // Run the TUI, passing both the app and the audio manager for audio visualization
    if let Err(e) = run_tui_with_audio(
        shared_app.clone(),
        audio_manager.clone(),
        app_check_connection,
        participants_clone,
    )
    .await
    {
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

    let mut audio_manager_for_cleanup = match Arc::try_unwrap(audio_manager) {
        Ok(mutex) => mutex.into_inner()?,
        Err(_) => {
            eprintln!("Could not get exclusive ownership of AudioStreamManager for cleanup");
            // Create a dummy audio manager for cleanup
            let mut manager = AudioStreamManager::new();
            manager.initialize()?;
            manager
        }
    };

    audio_manager_for_cleanup.stop_all_streams().await?;
    app_for_cleanup.shutdown().await?;

    Ok(())
}

// Modified version of run_tui that uses shared audio data
async fn run_tui_with_audio(
    app: Arc<Mutex<App>>,
    audio_manager: Arc<Mutex<AudioStreamManager>>,
    app_connection: Arc<App>,
    participants: Arc<Mutex<Vec<Participant>>>,
) -> io::Result<()> {
    // Initialize terminal
    let terminal_ui = ui::terminal_ui::TerminalUI::new();

    // Create a shared terminal_ui reference for settings manager
    let terminal_ui_arc = Arc::new(Mutex::new(terminal_ui));

    // Create settings manager
    let settings_manager = SettingsManager::new(
        Arc::clone(&app),
        Arc::clone(&audio_manager),
        Arc::clone(&terminal_ui_arc),
    );

    // Initialize the terminal UI
    {
        let mut terminal_ui = terminal_ui_arc.lock().unwrap();
        terminal_ui.initialize()?;
    }

    // Create an audio stream
    if let Ok(mut audio_manager_guard) = audio_manager.lock() {
        match audio_manager_guard
            .create_stream("default-session".to_string())
            .await
        {
            Ok(_stream) => {
                // Successfully created stream
            }
            Err(e) => {
                eprintln!("Error creating audio stream: {}", e);
            }
        }
    }

    // Main application loop
    let mut running = true;
    let tick_rate = Duration::from_millis(33); // ~30 FPS
    let mut last_tick = std::time::Instant::now();

    // For audio data updates
    let audio_update_rate = Duration::from_millis(100); // 10 Hz
    let mut last_audio_update = std::time::Instant::now();

    // For error rate limiting
    let error_throttle_duration = Duration::from_secs(1);
    let mut last_error_time = std::time::Instant::now();

    while running {
        // Manage locks carefully - only hold for short periods
        let mut terminal_ui = terminal_ui_arc.lock().unwrap();

        // Check if we're already in a session and set menu items accordingly
        let has_active_session = app_connection.has_active_connection().await;
        terminal_ui.update_menu_items(has_active_session);

        // Handle events and user input
        let input_timeout = Duration::from_millis(16);
        if let Some(event) = terminal_ui.poll_events(input_timeout)? {
            match event {
                crossterm::event::Event::Key(key_event) => {
                    // Handle Ctrl+C for exit
                    if key_event
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL)
                        && key_event.code == crossterm::event::KeyCode::Char('c')
                    {
                        running = false;
                        break;
                    }

                    if let Some(action) = terminal_ui.handle_key_event(key_event.code) {
                        // First handle internal actions like copy
                        if terminal_ui.handle_menu_action(action.clone()) {
                            continue;
                        }

                        let mut app_lock = app.lock().unwrap();
                        match action {
                            ui::MenuAction::Create => {
                                // Create a new session
                                if let Ok(session) = app_lock.create_p2p_session().await {
                                    terminal_ui
                                        .set_connection_link(Some(session.connection_link.clone()));
                                    terminal_ui.update_participants(session.participants.clone());
                                    // Update menu for active connection
                                    terminal_ui.update_menu_items(true);
                                }
                            }
                            ui::MenuAction::Join => {
                                terminal_ui.show_text_input_popup("Enter Session Link");
                            }
                            ui::MenuAction::Leave => {
                                if let Err(e) = app_lock.leave_session().await {
                                    eprintln!("Error leaving session: {}", e);
                                }

                                // Stop audio streams
                                let mut audio_manager_guard = audio_manager.lock().unwrap();
                                if let Err(e) = audio_manager_guard.stop_all_streams().await {
                                    eprintln!("Error stopping audio streams: {}", e);
                                }
                            }
                            ui::MenuAction::CopyLink => {
                                // Already handled by terminal UI
                            }
                            ui::MenuAction::Settings => {
                                // Release app_lock to avoid deadlock
                                drop(app_lock);

                                let settings_manager = SettingsManager::new(
                                    app.clone(),
                                    audio_manager.clone(),
                                    terminal_ui_arc.clone(),
                                );
                                settings_manager.show_settings_menu().await?;
                            }
                            ui::MenuAction::Quit => break,
                            ui::MenuAction::SettingsInputDevice => {
                                // This is handled by SettingsManager
                            }
                            ui::MenuAction::SettingsOutputDevice => {
                                // This is handled by SettingsManager
                            }
                            ui::MenuAction::SettingsTestSession => {
                                // This is handled by SettingsManager
                            }
                            ui::MenuAction::SettingsBack => {
                                // This is handled by TerminalUI
                            }
                            ui::MenuAction::DeviceSelect(_) => {
                                // This is handled by SettingsManager
                            }
                            ui::MenuAction::DeviceDefault => {
                                // This is handled by SettingsManager
                            }
                            ui::MenuAction::DeviceBack => {
                                // This is handled by TerminalUI
                            }
                            ui::MenuAction::TestSession => {
                                // Release app_lock to avoid deadlock
                                drop(app_lock);

                                let settings_manager = SettingsManager::new(
                                    app.clone(),
                                    audio_manager.clone(),
                                    terminal_ui_arc.clone(),
                                );
                                settings_manager.create_test_session().await?;
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Update audio-related data every 200ms
        if last_audio_update.elapsed() >= audio_update_rate {
            // Check if we have an active connection
            let has_connection = app_connection.has_active_connection().await;

            if has_connection {
                // Get current session participants from the app instance
                let current_session_participants =
                    if let Some(session) = app_connection.current_session() {
                        session.participants.clone()
                    } else {
                        vec![]
                    };

                if !current_session_participants.is_empty() {
                    // Update participants list with session participants
                    let mut participants_guard = participants.lock().unwrap();

                    // Start with the current user
                    let mut updated_participants = vec![participants_guard[0].clone()];

                    // Add other participants from the session
                    for participant in &current_session_participants {
                        if participant.name != "Me" {
                            updated_participants.push(participant.clone());
                        }
                    }

                    // Update the shared participants list
                    *participants_guard = updated_participants.clone();

                    // Let the audio manager know about updated participant positions
                    drop(participants_guard); // Release lock before taking another one
                    if let Ok(mut audio_manager_guard) = audio_manager.lock() {
                        if let Err(e) = audio_manager_guard.update_positions(&updated_participants)
                        {
                            let now = std::time::Instant::now();
                            if now.duration_since(last_error_time) > error_throttle_duration {
                                eprintln!("Error updating participant positions: {}", e);
                                last_error_time = now;
                            }
                        }

                        // Ensure each participant has an audio stream
                        for participant in &updated_participants {
                            if participant.name != "Me" {
                                if let Err(e) =
                                    audio_manager_guard.add_participant_stream(&participant.name)
                                {
                                    // Ignore errors if the stream already exists
                                    if !e.to_string().contains("already exists") {
                                        let now = std::time::Instant::now();
                                        if now.duration_since(last_error_time)
                                            > error_throttle_duration
                                        {
                                            eprintln!("Error adding participant stream: {}", e);
                                            last_error_time = now;
                                        }
                                    }
                                }
                            }
                        }

                        // Check if we're in a test session and get test audio data
                        let app_lock = app.lock().unwrap();
                        let is_test_session = app_lock
                            .current_session()
                            .map(|s| s.connection_link == "test-session")
                            .unwrap_or(false);

                        if is_test_session {
                            // Process audio for each test participant
                            for (i, participant) in updated_participants.iter().enumerate().skip(1)
                            {
                                if i <= 3 {
                                    // We have 3 test participants
                                    // Get the test audio data for this participant
                                    let test_audio = app_lock.get_test_participant_audio(i - 1);

                                    if !test_audio.is_empty() {
                                        // Process the audio data and add it to the participant's stream
                                        if let Err(e) = audio_manager_guard
                                            .process_remote_audio(&participant.name, &test_audio)
                                            .await
                                        {
                                            // Log errors but don't interrupt
                                            let now = std::time::Instant::now();
                                            if now.duration_since(last_error_time)
                                                > error_throttle_duration
                                            {
                                                eprintln!("Error processing test audio: {}", e);
                                                last_error_time = now;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            last_audio_update = std::time::Instant::now();
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

                // Get the latest audio data for visualization from the audio manager
                let audio_data = {
                    if let Ok(audio_manager) = audio_manager.lock() {
                        audio_manager.get_raw_capture_data()
                    } else {
                        Vec::new()
                    }
                };

                // Only update if we have data
                if !audio_data.is_empty() {
                    terminal_ui.update_audio_data(&audio_data);
                }
            }

            // Render UI
            let app_lock = app.lock().unwrap();
            terminal_ui.render(&app_lock)?;
            drop(app_lock);

            last_tick = std::time::Instant::now();
        }

        // Release the lock at the end of each iteration
        drop(terminal_ui);
    }

    // Shutdown terminal
    {
        let mut terminal_ui = terminal_ui_arc.lock().unwrap();
        terminal_ui.shutdown()?;
    }

    Ok(())
}
