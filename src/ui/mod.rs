// User Interface module
// Handles the terminal user interface

pub mod tui;
pub mod commands;
pub mod widgets;

// Re-export important types
pub use tui::Tui;
pub use commands::Command;