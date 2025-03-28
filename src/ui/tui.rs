use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    Terminal,
};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub struct UILayout {
    pub main_area: Rect,
    pub sidebar: Rect,
}

pub struct TerminalUI {
    terminal: Option<Terminal<CrosstermBackend<io::Stdout>>>,
    running: Arc<AtomicBool>,
}

impl TerminalUI {
    pub fn new() -> Self {
        Self {
            terminal: None,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_initialized(&self) -> bool {
        self.terminal.is_some()
    }

    pub fn initialize(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

        let backend = CrosstermBackend::new(stdout);
        self.terminal = Some(Terminal::new(backend)?);
        self.running.store(true, Ordering::SeqCst);

        Ok(())
    }

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

    pub fn create_layout(&self, width: u16, height: u16) -> UILayout {
        // Create a layout with a sidebar and main area
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(20), // Sidebar
                Constraint::Percentage(80), // Main area
            ])
            .split(Rect::new(0, 0, width, height));

        UILayout {
            sidebar: chunks[0],
            main_area: chunks[1],
        }
    }

    pub fn poll_events(&self, timeout: Duration) -> io::Result<Option<Event>> {
        if event::poll(timeout)? {
            return Ok(Some(event::read()?));
        }
        Ok(None)
    }

    pub fn draw<F>(&mut self, render_fn: F) -> io::Result<()>
    where
        F: FnOnce(&mut ratatui::Frame),
    {
        if let Some(terminal) = self.terminal.as_mut() {
            terminal.draw(render_fn)?;
        }
        Ok(())
    }

    pub fn get_size(&self) -> io::Result<Rect> {
        if let Some(terminal) = &self.terminal {
            return terminal.size();
        }
        Err(io::Error::new(
            io::ErrorKind::Other,
            "Terminal not initialized",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ui_creation() {
        let ui = TerminalUI::new();
        assert!(!ui.is_initialized());
    }

    #[test]
    fn test_ui_layout() {
        let ui = TerminalUI::new();
        let layout = ui.create_layout(80, 24);

        assert!(layout.main_area.width > 0);
        assert!(layout.main_area.height > 0);
        assert!(layout.sidebar.width > 0);
        assert!(layout.sidebar.height > 0);
    }
}
