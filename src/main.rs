mod app;
mod audio;
mod network;
mod ui;

use app::App;
use std::path::Path;

fn main() {
    let config_path = Path::new("config.toml");
    
    // Create a new app instance
    let mut app = if config_path.exists() {
        println!("Loading configuration from file");
        let mut app = App::new();
        if let Err(e) = app.load_config(config_path) {
            eprintln!("Failed to load configuration: {}", e);
        }
        app
    } else {
        println!("Using default configuration");
        App::new()
    };
    
    println!("Resonance audio communication application started");
    println!("Running with user: {}", app.config().username);
    
    // Here we would run the application main loop
    
    // Save configuration before exiting
    if let Err(e) = app.save_config(config_path) {
        eprintln!("Failed to save configuration: {}", e);
    }
    
    let shutdown_result = app.shutdown();
    if let Err(e) = shutdown_result {
        eprintln!("Error during shutdown: {}", e);
    }
}