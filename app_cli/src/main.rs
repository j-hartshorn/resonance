//! CLI application for room.rs

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use log::{debug, error, info, trace, warn};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Widget},
    Frame, Terminal,
};
use room_core::Error;
use settings_manager::ConfigManager;
use std::{
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

/// App state
struct App {
    /// Whether the app should exit
    should_quit: bool,
    /// Config manager instance
    config: ConfigManager,
}

impl App {
    fn new(config: ConfigManager) -> Self {
        Self {
            should_quit: false,
            config,
        }
    }

    /// Handle input events
    fn handle_event(&mut self, event: Event) -> Result<()> {
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Char('q') => self.should_quit = true,
                KeyCode::Esc => self.should_quit = true,
                _ => {}
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
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).init();
        debug!("Debug logging enabled");
    } else {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
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
    let mut app = App::new(config);

    // Main event loop
    let tick_rate = Duration::from_millis(100);
    let mut last_tick = Instant::now();

    loop {
        // Render the UI
        terminal.draw(|f| ui(f, &app))?;

        // Poll for events
        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_millis(0));

        if event::poll(timeout)? {
            let event = event::read()?;
            app.handle_event(event)?;
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }

        // Check if we should quit
        if app.should_quit {
            break;
        }
    }

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
    // Create three main sections with equal size
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Percentage(20), // Menu area
            Constraint::Percentage(40), // Peers area
            Constraint::Percentage(40), // Visualization area
        ])
        .split(f.size());

    // Menu area
    let menu = Paragraph::new(Text::from(vec![
        Line::from(vec![
            Span::styled("room.rs", Style::default().fg(Color::Green)),
            Span::raw(" - Secure, spatial audio chat"),
        ]),
        Line::raw(""),
        Line::raw("Press 'q' to quit"),
    ]))
    .block(Block::default().title("Menu").borders(Borders::ALL));
    f.render_widget(menu, chunks[0]);

    // Peers area (placeholder)
    let peers = Paragraph::new("No peers connected")
        .block(Block::default().title("Peers").borders(Borders::ALL));
    f.render_widget(peers, chunks[1]);

    // Visualization area (placeholder)
    let viz = Paragraph::new(format!("User: {}", app.config.settings().username)).block(
        Block::default()
            .title("Audio Visualization")
            .borders(Borders::ALL),
    );
    f.render_widget(viz, chunks[2]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;
    use mockall::predicate::*;
    use ratatui::backend::TestBackend;
    use settings_manager::Settings;
    use std::sync::Arc;

    // Mock ConfigManager for testing
    struct MockConfigManager {
        settings: Settings,
    }

    impl MockConfigManager {
        fn new() -> Self {
            Self {
                settings: Settings::default(),
            }
        }

        fn with_settings(settings: Settings) -> Self {
            Self { settings }
        }

        fn settings(&self) -> &Settings {
            &self.settings
        }
    }

    // Test app initialization
    #[test]
    fn test_app_init() {
        let mock_config = MockConfigManager::new();
        let app = App {
            should_quit: false,
            config: ConfigManager::with_file("non_existent_path.toml").unwrap(),
        };

        assert!(!app.should_quit);
        assert_eq!(app.config.settings().username, "Anonymous");
    }

    // Test quit event handling
    #[test]
    fn test_quit_event_handling() {
        let mock_config = MockConfigManager::new();
        let mut app = App {
            should_quit: false,
            config: ConfigManager::with_file("non_existent_path.toml").unwrap(),
        };

        // Test 'q' key press
        let q_event = Event::Key(KeyEvent::from(KeyCode::Char('q')));
        app.handle_event(q_event).unwrap();
        assert!(app.should_quit);

        // Reset and test escape key
        app.should_quit = false;
        let esc_event = Event::Key(KeyEvent::from(KeyCode::Esc));
        app.handle_event(esc_event).unwrap();
        assert!(app.should_quit);

        // Reset and test other key (should not quit)
        app.should_quit = false;
        let other_event = Event::Key(KeyEvent::from(KeyCode::Char('x')));
        app.handle_event(other_event).unwrap();
        assert!(!app.should_quit);
    }

    // Test UI rendering with test backend
    #[test]
    fn test_ui_rendering() {
        // Create a test backend with a specific size
        let backend = TestBackend::new(80, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        // Create app with custom settings
        let mut settings = Settings::default();
        settings.username = "TestUser".to_string();

        let app = App {
            should_quit: false,
            config: ConfigManager::with_file("non_existent_path.toml").unwrap(),
        };

        // Draw the UI
        terminal.draw(|f| ui(f, &app)).unwrap();

        // Get the buffer content as a string for easier testing
        let backend = terminal.backend();
        let buffer = backend.buffer();
        let content = format!("{:?}", buffer);

        // Check for section titles
        assert!(content.contains("Menu"));
        assert!(content.contains("Peers"));
        assert!(content.contains("Audio Visualization"));

        // Check for content
        assert!(content.contains("room.rs"));
        assert!(content.contains("Press 'q' to quit"));
        assert!(content.contains("No peers connected"));
        assert!(content.contains("User:"));
        assert!(content.contains("Anonymous"));
    }
}
