// Application coordinator module
// Coordinates the various components of the application

pub mod session;
pub mod config;

use anyhow::Result;
use crate::ui::tui::Tui;
use crate::app::session::SessionManager;
use crate::app::config::Config;

/// Main application runtime
pub async fn run(config: Config, tui: &mut Tui, session: &mut SessionManager) -> Result<()> {
    // Initialize components
    let _audio_system = crate::audio::setup(&config)?;
    let _network = crate::network::setup(&config).await?;
    
    // Application event loop
    while !tui.should_quit() {
        // Process UI events
        tui.handle_events()?;
        
        // Update session state
        session.update().await?;
        
        // Render UI
        tui.render(session)?;
    }
    
    // Clean shutdown
    Ok(())
}