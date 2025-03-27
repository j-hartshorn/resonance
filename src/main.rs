// resonance.rs - A high-fidelity spatial audio communication application
// Main application entry point

mod app;
mod ui;
mod network;
mod audio;

#[tokio::main]
async fn main() {
    println!("Starting resonance.rs...");
    
    // Initialize application configuration
    let config = app::config::Config::default();
    
    // Initialize the TUI
    let mut tui = ui::tui::Tui::new(&config).expect("Failed to initialize TUI");
    
    // Initialize the session manager
    let mut session = app::session::SessionManager::new(&config);
    
    // Application loop
    match app::run(config, &mut tui, &mut session).await {
        Ok(_) => println!("Application terminated successfully."),
        Err(e) => eprintln!("Application error: {}", e),
    }
}