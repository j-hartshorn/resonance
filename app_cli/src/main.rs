//! CLI application for room.rs

mod network_adapter;

#[cfg(test)]
mod tests;

use anyhow::Result;
use arboard::Clipboard;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use log::{debug, error, info, trace, warn};
use network_adapter::NetworkAdapter;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Widget},
    Frame, Terminal,
};
use room_core::{PeerId, RoomEvent, RoomId};
use settings_manager::ConfigManager;
use std::{
    collections::HashMap,
    io::{self, Stdout},
    time::{Duration, Instant},
};

/// room.rs - Secure, spatial audio chat
#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Args {
    /// Enable debug logging
    #[clap(short, long)]
    debug: bool,
}

/// Application state for the UI
enum AppState {
    /// Main menu
    MainMenu,
    /// Creating a room
    CreatingRoom,
    /// Joining a room
    JoiningRoom {
        /// The link input so far
        link: String,
    },
    /// In a room
    InRoom,
}

/// App state
struct App {
    /// Whether the app should quit
    should_quit: bool,
    /// Config manager instance
    config: ConfigManager,
    /// Network adapter for communication with room and network
    network_adapter: NetworkAdapter,
    /// Current application state
    state: AppState,
    /// Current room ID
    room_id: Option<RoomId>,
    /// List of peers in the room
    peers: HashMap<PeerId, String>,
    /// List of pending join requests
    pending_requests: HashMap<PeerId, ()>,
    /// Any status or error message to display
    status_message: Option<String>,
}

impl App {
    async fn new(config: ConfigManager) -> Result<Self> {
        // Create network adapter
        let network_adapter = NetworkAdapter::new().await;

        Ok(Self {
            should_quit: false,
            config,
            network_adapter,
            state: AppState::MainMenu,
            room_id: None,
            peers: HashMap::new(),
            pending_requests: HashMap::new(),
            status_message: None,
        })
    }

    // Helper method for tests - accepts a pre-configured network adapter
    #[cfg(test)]
    fn with_adapter(config: ConfigManager, adapter: NetworkAdapter) -> Self {
        Self {
            should_quit: false,
            config,
            network_adapter: adapter,
            state: AppState::MainMenu,
            room_id: None,
            peers: HashMap::new(),
            pending_requests: HashMap::new(),
            status_message: None,
        }
    }

    /// Handle input events
    async fn handle_event(&mut self, event: Event) -> Result<()> {
        if let Event::Key(key) = event {
            match self.state {
                AppState::MainMenu => self.handle_main_menu_input(key).await?,
                AppState::JoiningRoom { .. } => self.handle_joining_room_input(key).await?,
                AppState::InRoom => self.handle_in_room_input(key).await?,
                AppState::CreatingRoom => {} // No input handling needed during room creation
            }
        }
        Ok(())
    }

    /// Handle input when in the main menu
    async fn handle_main_menu_input(&mut self, key: event::KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('c') => {
                self.start_create_room().await?;
            }
            KeyCode::Char('j') => {
                self.state = AppState::JoiningRoom {
                    link: String::new(),
                };
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle input when joining a room
    async fn handle_joining_room_input(&mut self, key: event::KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.state = AppState::MainMenu;
                self.status_message = None;
            }
            KeyCode::Enter => {
                if let AppState::JoiningRoom { ref link } = self.state {
                    let link_clone = link.clone();
                    match self.network_adapter.join_room(&link_clone).await {
                        Ok(_) => {
                            self.state = AppState::InRoom;
                            self.status_message = Some(format!("Joining room via {}", link_clone));
                        }
                        Err(e) => {
                            self.status_message = Some(format!("Error: {}", e));
                        }
                    }
                }
            }
            KeyCode::Char(c) => {
                if let AppState::JoiningRoom { link } = &mut self.state {
                    link.push(c);
                }
            }
            KeyCode::Backspace => {
                if let AppState::JoiningRoom { link } = &mut self.state {
                    link.pop();
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle input when in a room
    async fn handle_in_room_input(&mut self, key: event::KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                // Leave room
                self.network_adapter.leave_room().await?;
                self.state = AppState::MainMenu;
                self.room_id = None;
                self.peers.clear();
                self.pending_requests.clear();
                self.status_message = None;
            }
            KeyCode::Char('a') => {
                // Approve first pending request (in a real app, would select which one)
                if let Some(peer_id) = self.pending_requests.keys().next().cloned() {
                    self.network_adapter.approve_join_request(peer_id).await?;
                    self.pending_requests.remove(&peer_id);
                    self.status_message = Some(format!("Approved join request from {}", peer_id));
                }
            }
            KeyCode::Char('d') => {
                // Deny first pending request (in a real app, would select which one)
                if let Some(peer_id) = self.pending_requests.keys().next().cloned() {
                    self.network_adapter
                        .deny_join_request(peer_id, Some("Denied by user".to_string()))
                        .await?;
                    self.pending_requests.remove(&peer_id);
                    self.status_message = Some(format!("Denied join request from {}", peer_id));
                }
            }
            KeyCode::Char('g') => match self.network_adapter.get_join_link().await {
                Ok(link) => match Clipboard::new() {
                    Ok(mut clipboard) => {
                        if let Err(e) = clipboard.set_text(link.clone()) {
                            self.status_message = Some(format!("Error copying link: {}", e));
                        } else {
                            self.status_message =
                                Some("Join link copied to clipboard!".to_string());
                        }
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Error accessing clipboard: {}", e));
                    }
                },
                Err(e) => {
                    self.status_message = Some(format!("Error getting join link: {}", e));
                }
            },
            _ => {}
        }
        Ok(())
    }

    /// Start creating a room
    async fn start_create_room(&mut self) -> Result<()> {
        self.state = AppState::CreatingRoom;
        self.status_message = Some("Creating room...".to_string());

        // Send create room command
        match self.network_adapter.create_room().await {
            Ok(_) => {
                self.status_message = Some("Room created, waiting for peers".to_string());
                self.state = AppState::InRoom;
            }
            Err(e) => {
                self.status_message = Some(format!("Error creating room: {}", e));
                self.state = AppState::MainMenu;
            }
        }

        Ok(())
    }

    /// Process room events
    async fn process_events(&mut self) -> Result<()> {
        while let Some(event) = self.network_adapter.try_recv_event().await {
            match event {
                RoomEvent::PeerAdded(peer_id) => {
                    let name = if peer_id == self.network_adapter.peer_id() {
                        "You".to_string()
                    } else {
                        format!("Peer {}", peer_id)
                    };
                    self.peers.insert(peer_id, name);
                }
                RoomEvent::PeerRemoved(peer_id) => {
                    self.peers.remove(&peer_id);
                }
                RoomEvent::JoinRequestReceived(peer_id) => {
                    self.pending_requests.insert(peer_id, ());
                    self.status_message = Some(format!("Join request from {}", peer_id));
                }
                RoomEvent::JoinRequestStatusChanged(peer_id, status) => {
                    self.status_message =
                        Some(format!("Join request from {} is now {}", peer_id, status));
                }
                RoomEvent::PeerListUpdated => {
                    // Just refresh the UI
                }
            }
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Configure logging based on debug flag
    if args.debug {
        let log_path = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".room_rs.log");

        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .expect("Failed to open log file");

        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
            .target(env_logger::Target::Pipe(Box::new(file)))
            .init();
        debug!("Debug logging enabled to file: {:?}", log_path);
    } else {
        let log_path = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".room_rs.log");

        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .expect("Failed to open log file");

        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
            .target(env_logger::Target::Pipe(Box::new(file)))
            .init();
    }

    info!("Starting room.rs CLI");

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let config = match ConfigManager::new() {
        Ok(config) => config,
        Err(e) => {
            error!("Failed to load config: {}", e);
            disable_raw_mode()?;
            execute!(
                terminal.backend_mut(),
                LeaveAlternateScreen,
                DisableMouseCapture
            )?;
            terminal.show_cursor()?;
            return Err(anyhow::anyhow!("Failed to load config: {}", e));
        }
    };
    let mut app = App::new(config).await?;

    // Main event loop
    let tick_rate = Duration::from_millis(100);
    let mut last_tick = Instant::now();

    loop {
        // Process room events
        app.process_events().await?;

        // Render the UI
        terminal.draw(|f| ui(f, &app))?;

        // Poll for events
        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_millis(0));

        if event::poll(timeout)? {
            let event = event::read()?;
            app.handle_event(event).await?;
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }

        // Check if we should quit
        if app.should_quit {
            break;
        }
    }

    // Shutdown audio system
    app.network_adapter.shutdown();

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    info!("Exiting room.rs CLI");

    Ok(())
}

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(3), // Top status/menu area
                Constraint::Min(0),    // Main content (peers/viz)
                Constraint::Length(1), // Bottom help text
            ]
            .as_ref(),
        )
        .split(f.size());

    // --- Top Status/Menu ---
    let status_text = match &app.state {
        AppState::MainMenu => "Main Menu".to_string(),
        AppState::CreatingRoom => "Creating Room...".to_string(),
        AppState::JoiningRoom { link } => format!("Joining Room | Link: {}", link),
        AppState::InRoom => format!(
            "In Room: {} | Peers: {}",
            app.room_id.map_or_else(
                || "Unknown".to_string(),
                |id| format!("{}", id) // Adjust formatting as needed
            ),
            app.peers.len()
        ),
    };
    let status_paragraph =
        Paragraph::new(status_text).block(Block::default().borders(Borders::ALL).title("Status"));
    f.render_widget(status_paragraph, chunks[0]);

    // --- Main Content ---
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
        .split(chunks[1]);

    // Peer List
    let peer_items: Vec<ListItem> = app
        .peers
        .iter()
        .map(|(id, name)| {
            let content = format!("{} ({})", name, id);
            ListItem::new(Span::raw(content))
        })
        .collect();
    let peer_list = List::new(peer_items)
        .block(Block::default().borders(Borders::ALL).title("Peers"))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol("> ");
    f.render_widget(peer_list, main_chunks[0]);

    // Placeholder for Visualization
    let viz_block = Block::default()
        .borders(Borders::ALL)
        .title("Visualization (Placeholder)");
    f.render_widget(viz_block, main_chunks[1]);

    // --- Bottom Help Text / Status Message ---
    let mut help_spans = vec![];
    match app.state {
        AppState::MainMenu => {
            help_spans.push(Span::styled("[C]", Style::default().fg(Color::Yellow)));
            help_spans.push(Span::raw("reate Room | "));
            help_spans.push(Span::styled("[J]", Style::default().fg(Color::Yellow)));
            help_spans.push(Span::raw("oin Room | "));
            help_spans.push(Span::styled("[Q]", Style::default().fg(Color::Yellow)));
            help_spans.push(Span::raw("uit"));
        }
        AppState::JoiningRoom { .. } => {
            help_spans.push(Span::raw("Enter Link | "));
            help_spans.push(Span::styled("[Enter]", Style::default().fg(Color::Yellow)));
            help_spans.push(Span::raw(" to Join | "));
            help_spans.push(Span::styled("[Esc]", Style::default().fg(Color::Yellow)));
            help_spans.push(Span::raw(" to Cancel"));
        }
        AppState::InRoom => {
            if !app.pending_requests.is_empty() {
                help_spans.push(Span::styled("[A]", Style::default().fg(Color::Green)));
                help_spans.push(Span::raw("pprove Join | "));
                help_spans.push(Span::styled("[D]", Style::default().fg(Color::Red)));
                help_spans.push(Span::raw("eny Join | "));
            }
            help_spans.push(Span::styled("[G]", Style::default().fg(Color::Yellow))); // Add G option
            help_spans.push(Span::raw(" Copy Join Link | ")); // Add G option text
            help_spans.push(Span::styled("[Esc]", Style::default().fg(Color::Yellow)));
            help_spans.push(Span::raw(" to Leave Room"));
        }
        AppState::CreatingRoom => {
            help_spans.push(Span::raw("Creating room..."));
        }
    }

    let bottom_text = if let Some(msg) = &app.status_message {
        Line::from(vec![
            Span::styled("Status: ", Style::default().fg(Color::Cyan)),
            Span::raw(msg),
        ])
    } else {
        Line::from(help_spans)
    };

    let help_paragraph = Paragraph::new(bottom_text);
    f.render_widget(help_paragraph, chunks[2]);
}
