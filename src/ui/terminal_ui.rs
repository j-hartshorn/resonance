use clipboard::{ClipboardContext, ClipboardProvider};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Widget},
    Frame, Terminal,
};
use std::{
    io::{self, stdout},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::app::App;
use crate::audio;
use crate::ui::widgets::{AudioVisualizationWidget, Participant, ParticipantListWidget};

/// Structure representing the layout of the UI
#[derive(Debug, Clone, Copy)]
pub struct AppLayout {
    pub menu_area: Rect,         // Top left - Menu options
    pub participants_area: Rect, // Top right - User list
    pub audio_area: Rect,        // Bottom - Audio visualization
    pub status_bar: Rect,        // Bottom bar - Connection info and status
}

/// Represents a selectable menu item
#[derive(Debug, Clone)]
pub struct MenuItem {
    pub label: String,
    pub action: MenuAction,
}

/// Represents possible menu actions
#[derive(Debug, Clone)]
pub enum MenuAction {
    Join,
    Leave,
    CopyLink,
    Quit,
}

/// Represents UI notification state
#[derive(Debug, Clone)]
struct Notification {
    message: String,
    start_time: Instant,
    duration: Duration,
}

/// Main UI controller that manages terminal rendering
pub struct TerminalUI {
    terminal: Option<Terminal<CrosstermBackend<io::Stdout>>>,
    running: Arc<AtomicBool>,
    menu_items: Vec<MenuItem>,
    menu_state: ListState,
    participants: Arc<Mutex<Vec<Participant>>>,
    audio_visualizer: AudioVisualizationWidget,
    connection_link: Arc<Mutex<Option<String>>>,
    notification: Option<Notification>,
    clipboard: Option<ClipboardContext>,
}

impl TerminalUI {
    pub fn new() -> Self {
        // Create default menu items
        let menu_items = vec![
            MenuItem {
                label: "Join Session".to_string(),
                action: MenuAction::Join,
            },
            MenuItem {
                label: "Leave Session".to_string(),
                action: MenuAction::Leave,
            },
            MenuItem {
                label: "Copy Link".to_string(),
                action: MenuAction::CopyLink,
            },
            MenuItem {
                label: "Quit".to_string(),
                action: MenuAction::Quit,
            },
        ];

        // Initialize with empty menu selection
        let mut menu_state = ListState::default();
        menu_state.select(Some(0)); // Select the first item by default

        // Initialize clipboard
        let clipboard = ClipboardProvider::new().ok();

        Self {
            terminal: None,
            running: Arc::new(AtomicBool::new(false)),
            menu_items,
            menu_state,
            participants: Arc::new(Mutex::new(Vec::new())),
            audio_visualizer: AudioVisualizationWidget::new(),
            connection_link: Arc::new(Mutex::new(None)),
            notification: None,
            clipboard,
        }
    }

    /// Checks if the terminal UI is initialized
    pub fn is_initialized(&self) -> bool {
        self.terminal.is_some()
    }

    /// Initializes the terminal UI
    pub fn initialize(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

        let backend = CrosstermBackend::new(stdout);
        self.terminal = Some(Terminal::new(backend)?);
        self.running.store(true, Ordering::SeqCst);

        Ok(())
    }

    /// Shuts down the terminal UI
    pub fn shutdown(&mut self) -> io::Result<()> {
        if let Some(terminal) = self.terminal.as_mut() {
            disable_raw_mode()?;
            execute!(
                terminal.backend_mut(),
                LeaveAlternateScreen,
                DisableMouseCapture
            )?;
            terminal.show_cursor()?;
        }
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Creates the application layout
    pub fn create_layout(&self, area: Rect) -> AppLayout {
        // First split for top content and status bar
        let vertical_split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),    // Main content area
                Constraint::Length(3), // Status bar at bottom
            ])
            .split(area);

        // Split top area into two vertical sections
        let top_areas = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(45), // Top section (menu and participants)
                Constraint::Percentage(55), // Bottom section (audio visualization)
            ])
            .split(vertical_split[0]);

        // Split the top section horizontally for menu and participants
        let top_split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(top_areas[0]);

        AppLayout {
            menu_area: top_split[0],
            participants_area: top_split[1],
            audio_area: top_areas[1],
            status_bar: vertical_split[1],
        }
    }

    /// Updates the list of participants
    pub fn update_participants(&self, participants: Vec<Participant>) {
        let mut lock = self.participants.lock().unwrap();
        *lock = participants;
    }

    /// Updates the audio visualization data
    pub fn update_audio_data(&self, data: &[f32]) {
        self.audio_visualizer.update_data(data);
    }

    /// Sets the connection link for display
    pub fn set_connection_link(&self, link: Option<String>) {
        let mut lock = self.connection_link.lock().unwrap();
        *lock = link;
    }

    /// Copy text to clipboard
    fn copy_to_clipboard(&mut self, text: &str) -> bool {
        if let Some(clipboard) = &mut self.clipboard {
            if clipboard.set_contents(text.to_owned()).is_ok() {
                self.show_notification(
                    "Link copied to clipboard!".to_string(),
                    Duration::from_secs(2),
                );
                return true;
            }
        }
        self.show_notification("Failed to copy link!".to_string(), Duration::from_secs(2));
        false
    }

    /// Show a notification message
    fn show_notification(&mut self, message: String, duration: Duration) {
        self.notification = Some(Notification {
            message,
            start_time: Instant::now(),
            duration,
        });
    }

    /// Update notification state (remove if expired)
    fn update_notification(&mut self) {
        if let Some(notification) = &self.notification {
            if notification.start_time.elapsed() >= notification.duration {
                self.notification = None;
            }
        }
    }

    /// Handles key events
    pub fn handle_key_event(&mut self, key: KeyCode) -> Option<MenuAction> {
        match key {
            KeyCode::Up => {
                // Move menu selection up
                let current = self.menu_state.selected().unwrap_or(0);
                if current > 0 {
                    self.menu_state.select(Some(current - 1));
                }
                None
            }
            KeyCode::Down => {
                // Move menu selection down
                let current = self.menu_state.selected().unwrap_or(0);
                if current < self.menu_items.len() - 1 {
                    self.menu_state.select(Some(current + 1));
                }
                None
            }
            KeyCode::Enter => {
                // Execute selected menu action
                if let Some(selected) = self.menu_state.selected() {
                    Some(self.menu_items[selected].action.clone())
                } else {
                    None
                }
            }
            KeyCode::Char('q') => Some(MenuAction::Quit),
            KeyCode::Char('j') => Some(MenuAction::Join),
            KeyCode::Char('l') => Some(MenuAction::Leave),
            KeyCode::Char('c') => Some(MenuAction::CopyLink),
            _ => None,
        }
    }

    /// Handle menu actions
    pub fn handle_menu_action(&mut self, action: MenuAction) -> bool {
        match action {
            MenuAction::CopyLink => {
                let connection_link = self.connection_link.lock().unwrap().clone();
                if let Some(link) = connection_link {
                    self.copy_to_clipboard(&link);
                } else {
                    self.show_notification(
                        "No active link to copy".to_string(),
                        Duration::from_secs(2),
                    );
                }
                false // Don't exit after copying
            }
            _ => false, // Let other actions be handled externally
        }
    }

    /// Polls for terminal events
    pub fn poll_events(&self, timeout: Duration) -> io::Result<Option<Event>> {
        if event::poll(timeout)? {
            return Ok(Some(event::read()?));
        }
        Ok(None)
    }

    /// Renders the UI
    pub fn render(&mut self, _app: &App) -> io::Result<()> {
        // Update notification state
        self.update_notification();

        if let Some(terminal) = self.terminal.as_mut() {
            // Create local copies of all the data we need
            let menu_items = self.menu_items.clone();
            let mut menu_state = self.menu_state.clone();
            let participants = self.participants.lock().unwrap().clone();
            let connection_link = self.connection_link.lock().unwrap().clone();
            let audio_visualizer = self.audio_visualizer.clone();
            let notification = self.notification.clone();

            // Create a local Layout function
            let create_layout = |area: Rect| -> AppLayout {
                // First split for top content and status bar
                let vertical_split = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(5),    // Main content area
                        Constraint::Length(3), // Status bar at bottom
                    ])
                    .split(area);

                // Split top area into two vertical sections
                let top_areas = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Percentage(45), // Top section (menu and participants)
                        Constraint::Percentage(55), // Bottom section (audio visualization)
                    ])
                    .split(vertical_split[0]);

                // Split the top section horizontally for menu and participants
                let top_split = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
                    .split(top_areas[0]);

                AppLayout {
                    menu_area: top_split[0],
                    participants_area: top_split[1],
                    audio_area: top_areas[1],
                    status_bar: vertical_split[1],
                }
            };

            terminal.draw(move |frame| {
                let area = frame.size();
                let layout = create_layout(area);

                // Render menu area (top left)
                let menu_items: Vec<ListItem> = menu_items
                    .iter()
                    .map(|item| {
                        ListItem::new(Line::from(vec![Span::styled(
                            &item.label,
                            Style::default().fg(Color::White),
                        )]))
                    })
                    .collect();

                let menu = List::new(menu_items)
                    .block(Block::default().title("Menu").borders(Borders::ALL))
                    .highlight_style(Style::default().fg(Color::Yellow));

                frame.render_stateful_widget(menu, layout.menu_area, &mut menu_state);

                // Render participants list (top right)
                let participant_items: Vec<ListItem> = participants
                    .iter()
                    .map(|p| {
                        let style = if p.is_speaking {
                            Style::default().fg(Color::Green)
                        } else {
                            Style::default().fg(Color::White)
                        };

                        ListItem::new(Line::from(vec![Span::styled(&p.name, style)]))
                    })
                    .collect();

                let participant_list = List::new(participant_items)
                    .block(Block::default().title("Participants").borders(Borders::ALL));

                frame.render_widget(participant_list, layout.participants_area);

                // Render audio visualization (bottom)
                frame.render_widget(audio_visualizer, layout.audio_area);

                // Render status bar (very bottom - connection link and status)
                let status_text = match &connection_link {
                    Some(link) => {
                        format!("Join Link: {} (Press 'c' to copy)", link)
                    }
                    None => "Not connected - use Join to create a session".to_string(),
                };

                let status_bar = Paragraph::new(status_text)
                    .style(Style::default())
                    .block(Block::default().borders(Borders::ALL).title("Status"));

                frame.render_widget(status_bar, layout.status_bar);

                // If there's an active notification, render it as an overlay
                if let Some(notif) = notification {
                    // Create a centered popup for the notification
                    let notif_width = notif.message.len() as u16 + 4; // Add padding
                    let notif_height = 3;
                    let notif_x = (area.width - notif_width) / 2;
                    let notif_y = (area.height - notif_height) / 2;

                    let notif_area = Rect::new(notif_x, notif_y, notif_width, notif_height);

                    let notification_widget = Paragraph::new(notif.message)
                        .style(Style::default().fg(Color::White))
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .style(Style::default().bg(Color::DarkGray)),
                        );

                    frame.render_widget(notification_widget, notif_area);
                }
            })?;
        }
        Ok(())
    }
}

use std::sync::atomic::{AtomicBool, Ordering};

/// Run the TUI application
pub async fn run_tui(app: Arc<Mutex<App>>) -> io::Result<()> {
    // Initialize terminal
    let mut terminal_ui = TerminalUI::new();
    terminal_ui.initialize()?;

    // Main event loop
    let tick_rate = Duration::from_millis(33); // ~30 FPS
    let mut last_tick = Instant::now();

    loop {
        // Check if enough time has passed for a frame update
        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        // Poll for events
        if let Some(event) = terminal_ui.poll_events(timeout)? {
            match event {
                Event::Key(key_event) => {
                    // Handle Ctrl+C for exit
                    if key_event.modifiers.contains(KeyModifiers::CONTROL)
                        && key_event.code == KeyCode::Char('c')
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
                            MenuAction::Join => {
                                // Prompt user for join link or create new session
                                // For this example, we'll just create a new session
                                if let Ok(session) = app_lock.create_p2p_session().await {
                                    terminal_ui
                                        .set_connection_link(Some(session.connection_link.clone()));

                                    // Update participants
                                    terminal_ui.update_participants(session.participants.clone());
                                }
                            }
                            MenuAction::Leave => {
                                if app_lock.has_active_connection().await {
                                    let _ = app_lock.leave_session().await;
                                    terminal_ui.set_connection_link(None);
                                    terminal_ui.update_participants(vec![]);
                                }
                            }
                            MenuAction::CopyLink => {
                                // Already handled in handle_menu_action
                            }
                            MenuAction::Quit => break,
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

                // Note: Audio data is now updated directly from the microphone input
                // in the main thread, so we don't need to generate test audio here
            }

            // Render UI
            terminal_ui.render(&app.lock().unwrap())?;

            last_tick = Instant::now();
        }
    }

    // Shutdown terminal
    terminal_ui.shutdown()?;

    Ok(())
}
