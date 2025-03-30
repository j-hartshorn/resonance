use crossterm::event::{Event, KeyCode};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::app::App;
use crate::audio::{AudioDevice, AudioStreamManager};
use crate::ui::terminal_ui::{MenuAction, TerminalUI};

/// SettingsManager handles all settings-related UI and logic
pub struct SettingsManager {
    app: Arc<Mutex<App>>,
    audio_manager: Arc<Mutex<AudioStreamManager>>,
    terminal_ui: Arc<Mutex<TerminalUI>>,
}

impl SettingsManager {
    /// Create a new SettingsManager
    pub fn new(
        app: Arc<Mutex<App>>,
        audio_manager: Arc<Mutex<AudioStreamManager>>,
        terminal_ui: Arc<Mutex<TerminalUI>>,
    ) -> Self {
        Self {
            app,
            audio_manager,
            terminal_ui,
        }
    }

    /// Display and handle the settings menu and subsequent navigation
    pub async fn show_settings_menu(&self) -> std::io::Result<()> {
        // Show settings submenu by updating the UI
        {
            let mut terminal_ui = self.terminal_ui.lock().unwrap();
            terminal_ui.show_settings_menu();
        }

        // Process input for settings menu
        self.handle_settings_navigation().await
    }

    /// Handle navigation within the settings menu
    async fn handle_settings_navigation(&self) -> std::io::Result<()> {
        loop {
            let input_timeout = Duration::from_millis(16);

            // Render the current state
            {
                let app_lock = self.app.lock().unwrap();
                let mut terminal_ui = self.terminal_ui.lock().unwrap();
                terminal_ui.render(&app_lock)?;
                drop(terminal_ui);
                drop(app_lock);
            }

            // Wait for and process input
            let mut terminal_ui = self.terminal_ui.lock().unwrap();
            let event_result = terminal_ui.poll_events(input_timeout);

            // Process input
            match event_result {
                Ok(Some(event)) => {
                    if let Event::Key(key_event) = event {
                        match key_event.code {
                            // Pass all key events to the terminal UI first
                            key => {
                                // Handle navigation & selection
                                if let Some(action) = terminal_ui.handle_key_event(key) {
                                    match action {
                                        MenuAction::SettingsInputDevice => {
                                            drop(terminal_ui);
                                            self.show_input_device_menu().await?;
                                            return Ok(());
                                        }
                                        MenuAction::SettingsOutputDevice => {
                                            drop(terminal_ui);
                                            self.show_output_device_menu().await?;
                                            return Ok(());
                                        }
                                        MenuAction::SettingsTestSession => {
                                            drop(terminal_ui);
                                            self.create_test_session().await?;
                                            return Ok(());
                                        }
                                        MenuAction::SettingsBack | MenuAction::Quit => {
                                            // Return to main menu
                                            terminal_ui.return_to_previous_menu();
                                            return Ok(());
                                        }
                                        _ => {}
                                    }
                                } else if key == KeyCode::Esc {
                                    // Special case for Escape key
                                    terminal_ui.return_to_previous_menu();
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    return Err(e);
                }
                _ => {}
            }

            drop(terminal_ui);

            // Small sleep to prevent CPU hogging
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// Display and handle the input device selection menu
    async fn show_input_device_menu(&self) -> std::io::Result<()> {
        // Get available input devices
        let input_devices = {
            let audio_manager = self.audio_manager.lock().unwrap();
            audio_manager.get_input_devices()
        };

        let current_device = {
            let audio_manager = self.audio_manager.lock().unwrap();
            audio_manager.current_input_device()
        };

        // Show device menu
        {
            let mut terminal_ui = self.terminal_ui.lock().unwrap();
            terminal_ui.show_device_menu(
                true, // is_input
                &input_devices,
                current_device.as_ref(),
            );
        }

        // Process device selection
        self.handle_device_selection(input_devices, true).await
    }

    /// Display and handle the output device selection menu
    async fn show_output_device_menu(&self) -> std::io::Result<()> {
        // Get available output devices
        let output_devices = {
            let audio_manager = self.audio_manager.lock().unwrap();
            audio_manager.get_output_devices()
        };

        let current_device = {
            let audio_manager = self.audio_manager.lock().unwrap();
            audio_manager.current_output_device()
        };

        // Show device menu
        {
            let mut terminal_ui = self.terminal_ui.lock().unwrap();
            terminal_ui.show_device_menu(
                false, // is_input
                &output_devices,
                current_device.as_ref(),
            );
        }

        // Process device selection
        self.handle_device_selection(output_devices, false).await
    }

    /// Handle device selection input
    async fn handle_device_selection(
        &self,
        devices: Vec<AudioDevice>,
        is_input: bool,
    ) -> std::io::Result<()> {
        loop {
            let input_timeout = Duration::from_millis(16);

            // Render the current state
            {
                let app_lock = self.app.lock().unwrap();
                let mut terminal_ui = self.terminal_ui.lock().unwrap();
                terminal_ui.render(&app_lock)?;
                drop(terminal_ui);
                drop(app_lock);
            }

            // Wait for and process input
            let mut terminal_ui = self.terminal_ui.lock().unwrap();
            let event_result = terminal_ui.poll_events(input_timeout);

            // Process device selection input
            match event_result {
                Ok(Some(event)) => {
                    if let Event::Key(key_event) = event {
                        match key_event.code {
                            // Pass all key events to the terminal UI first
                            key => {
                                // Handle navigation & selection
                                if let Some(action) = terminal_ui.handle_key_event(key) {
                                    match action {
                                        MenuAction::DeviceDefault => {
                                            drop(terminal_ui);
                                            if let Err(e) = self
                                                .select_default_device(devices.clone(), is_input)
                                                .await
                                            {
                                                eprintln!("Error setting default device: {}", e);
                                            }

                                            // Return to previous menu after selection
                                            let mut terminal_ui = self.terminal_ui.lock().unwrap();
                                            terminal_ui.return_to_previous_menu();
                                            return Ok(());
                                        }
                                        MenuAction::DeviceSelect(index) => {
                                            drop(terminal_ui);
                                            if let Err(e) = self
                                                .set_audio_device(index, devices.clone(), is_input)
                                                .await
                                            {
                                                eprintln!("Error setting device: {}", e);
                                            }

                                            // Return to previous menu after selection
                                            let mut terminal_ui = self.terminal_ui.lock().unwrap();
                                            terminal_ui.return_to_previous_menu();
                                            return Ok(());
                                        }
                                        MenuAction::DeviceBack | MenuAction::Quit => {
                                            // Return to settings menu
                                            terminal_ui.return_to_previous_menu();
                                            return Ok(());
                                        }
                                        _ => {}
                                    }
                                } else if key == KeyCode::Esc {
                                    // Special case for Escape key
                                    terminal_ui.return_to_previous_menu();
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    return Err(e);
                }
                _ => {}
            }

            drop(terminal_ui);

            // Small sleep to prevent CPU hogging
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// Select the default device (index 0)
    async fn select_default_device(
        &self,
        devices: Vec<AudioDevice>,
        is_input: bool,
    ) -> std::io::Result<()> {
        if let Some(default_device) = devices.first() {
            self.set_audio_device(0, devices, is_input).await?;
        } else {
            let mut terminal_ui = self.terminal_ui.lock().unwrap();
            terminal_ui.show_notification(
                "No default device available".to_string(),
                Duration::from_secs(2),
            );
        }
        Ok(())
    }

    /// Set the selected audio device in the manager and config
    async fn set_audio_device(
        &self,
        index: usize,
        devices: Vec<AudioDevice>,
        is_input: bool,
    ) -> std::io::Result<()> {
        if index < devices.len() {
            let selected_device = &devices[index];
            let mut audio_manager = self.audio_manager.lock().unwrap();

            let result = if is_input {
                audio_manager
                    .set_input_device(selected_device.clone())
                    .await
            } else {
                audio_manager.set_output_device(selected_device.clone())
            };

            if let Err(e) = result {
                let mut terminal_ui = self.terminal_ui.lock().unwrap();
                terminal_ui.show_notification(
                    format!(
                        "Error setting {} device: {}",
                        if is_input { "input" } else { "output" },
                        e
                    ),
                    Duration::from_secs(3),
                );
            } else {
                // Update app config
                drop(audio_manager);
                self.update_device_config(selected_device, is_input)?;

                let mut terminal_ui = self.terminal_ui.lock().unwrap();
                terminal_ui.show_notification(
                    format!(
                        "Set {} device to: {}",
                        if is_input { "input" } else { "output" },
                        selected_device.name
                    ),
                    Duration::from_secs(2),
                );
            }
        } else {
            let mut terminal_ui = self.terminal_ui.lock().unwrap();
            terminal_ui.show_notification(
                "Invalid device selection".to_string(),
                Duration::from_secs(2),
            );
        }

        Ok(())
    }

    /// Update the device configuration in the app config
    fn update_device_config(&self, device: &AudioDevice, is_input: bool) -> std::io::Result<()> {
        let mut app_lock = self.app.lock().unwrap();
        let mut config = app_lock.config().clone();

        if is_input {
            config.input_device = Some(device.id.clone());
        } else {
            config.output_device = Some(device.id.clone());
        }

        app_lock.update_config(config);
        Ok(())
    }

    /// Create a test session
    pub async fn create_test_session(&self) -> std::io::Result<()> {
        let mut app_lock = self.app.lock().unwrap();
        let mut terminal_ui = self.terminal_ui.lock().unwrap();

        if let Ok(session) = app_lock.create_test_session().await {
            terminal_ui.update_participants(session.participants.clone());
            terminal_ui.set_connection_link(Some("Test Session".to_string()));
            terminal_ui.update_menu_items(true);
            terminal_ui.show_notification(
                "Test session created with 3 simulated participants".to_string(),
                Duration::from_secs(2),
            );
        } else {
            terminal_ui.show_notification(
                "Failed to create test session".to_string(),
                Duration::from_secs(2),
            );
        }

        Ok(())
    }
}
