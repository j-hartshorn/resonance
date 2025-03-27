// Terminal User Interface implementation
// Provides the TUI for the application

use std::io;
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::Span,
    widgets::{Block, Borders, Paragraph},
    Terminal,
};

use crate::app::config::Config;
use crate::app::session::SessionManager;
use crate::ui::commands::Command;

pub struct Tui {
    terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
    should_quit: bool,
    config: Config,
}

impl Tui {
    pub fn new(config: &Config) -> Result<Self> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        stdout.execute(EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        
        Ok(Self {
            terminal,
            should_quit: false,
            config: config.clone(),
        })
    }
    
    pub fn should_quit(&self) -> bool {
        self.should_quit
    }
    
    pub fn handle_events(&mut self) -> Result<()> {
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => {
                            self.should_quit = true;
                        }
                        // More key handling would go here
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }
    
    pub fn render(&mut self, session: &SessionManager) -> Result<()> {
        self.terminal.draw(|f| {
            let size = f.size();
            
            // Create the layout
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(3),  // Title
                    Constraint::Min(10),    // Main content
                    Constraint::Length(3),  // Status bar
                ].as_ref())
                .split(size);
            
            // Render title
            let title = Paragraph::new("resonance.rs")
                .style(Style::default().fg(Color::Cyan))
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(title, chunks[0]);
            
            // Render main content (participants)
            let participants_block = Block::default()
                .title("Participants")
                .borders(Borders::ALL);
            f.render_widget(participants_block, chunks[1]);
            
            // Render status bar
            let status_text = if session.is_connected() {
                "Connected | Press 'q' to quit"
            } else {
                "Disconnected | Press 'q' to quit"
            };
            let status = Paragraph::new(Span::raw(status_text))
                .style(Style::default().fg(Color::White))
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(status, chunks[2]);
        })?;
        
        Ok(())
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        // Restore terminal
        let _ = disable_raw_mode();
        let _ = self.terminal.backend_mut().execute(LeaveAlternateScreen);
    }
}