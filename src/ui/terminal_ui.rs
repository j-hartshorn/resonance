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
use crate::ui::qr_code::generate_qr_code;
use crate::ui::widgets::{AudioVisualizationWidget, Participant, ParticipantListWidget};

/// Structure representing the quadrant layout of the UI
#[derive(Debug, Clone, Copy)]
pub struct QuadrantLayout {
    pub top_left: Rect,     // Menu options
    pub top_right: Rect,    // User list
    pub bottom_left: Rect,  // QR code and join link
    pub bottom_right: Rect, // Audio visualization
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
    Quit,
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
                label: "Quit".to_string(),
                action: MenuAction::Quit,
            },
        ];

        // Initialize with empty menu selection
        let mut menu_state = ListState::default();
        menu_state.select(Some(0)); // Select the first item by default

        Self {
            terminal: None,
            running: Arc::new(AtomicBool::new(false)),
            menu_items,
            menu_state,
            participants: Arc::new(Mutex::new(Vec::new())),
            audio_visualizer: AudioVisualizationWidget::new(),
            connection_link: Arc::new(Mutex::new(None)),
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

    /// Creates a quadrant layout
    pub fn create_layout(&self, area: Rect) -> QuadrantLayout {
        // First split the screen into top and bottom
        let vertical_split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        // Then split each half horizontally
        let top_split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(vertical_split[0]);

        let bottom_split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(vertical_split[1]);

        QuadrantLayout {
            top_left: top_split[0],
            top_right: top_split[1],
            bottom_left: bottom_split[0],
            bottom_right: bottom_split[1],
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
            _ => None,
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
        if let Some(terminal) = self.terminal.as_mut() {
            // Create local copies of all the data we need
            let menu_items = self.menu_items.clone();
            let mut menu_state = self.menu_state.clone();
            let participants = self.participants.lock().unwrap().clone();
            let connection_link = self.connection_link.lock().unwrap().clone();
            let audio_visualizer = self.audio_visualizer.clone();

            // Create a local Layout function
            let create_layout = |area: Rect| -> QuadrantLayout {
                // First split the screen into top and bottom
                let vertical_split = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(area);

                // Then split each half horizontally
                let top_split = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(vertical_split[0]);

                let bottom_split = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(vertical_split[1]);

                QuadrantLayout {
                    top_left: top_split[0],
                    top_right: top_split[1],
                    bottom_left: bottom_split[0],
                    bottom_right: bottom_split[1],
                }
            };

            terminal.draw(move |frame| {
                let area = frame.size();
                let layout = create_layout(area);

                // Render top-left quadrant (menu options)
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

                frame.render_stateful_widget(menu, layout.top_left, &mut menu_state);

                // Render top-right quadrant (participant list)
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

                frame.render_widget(participant_list, layout.top_right);

                // Render bottom-left quadrant (QR code and connection info)
                let content = if let Some(link) = &connection_link {
                    // Generate QR code
                    let qr_code = generate_qr_code(link)
                        .unwrap_or_else(|_| "Failed to generate QR code".to_string());

                    format!("Join Link:\n{}\n\n{}", link, qr_code)
                } else {
                    "No active session\nUse menu to join or create a session".to_string()
                };

                let connection_info = Paragraph::new(content).block(
                    Block::default()
                        .title("Connection Info")
                        .borders(Borders::ALL),
                );

                frame.render_widget(connection_info, layout.bottom_left);

                // Render bottom-right quadrant (audio visualization)
                frame.render_widget(audio_visualizer, layout.bottom_right);
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
                    if key_event.modifiers.contains(KeyModifiers::CONTROL)
                        && key_event.code == KeyCode::Char('c')
                    {
                        break;
                    }

                    if let Some(action) = terminal_ui.handle_key_event(key_event.code) {
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

                // Usually you would get audio data here
                // For example: terminal_ui.update_audio_data(audio_data);
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
