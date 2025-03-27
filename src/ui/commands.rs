// Command processing module
// Handles parsing and execution of user commands

use anyhow::Result;
use crate::app::session::SessionManager;

#[derive(Debug, Clone)]
pub enum Command {
    Help,
    CreateSession(String),          // with username
    JoinSession(String, String),    // with session link and username
    LeaveSession,
    Quit,
    SetPosition(f32, f32, f32),     // x, y, z coordinates
    Mute(bool),                     // mute/unmute self
    SetVolume(String, f32),         // set volume for participant
    Unknown(String),                // unknown command
}

pub fn parse_command(input: &str) -> Command {
    let input = input.trim();
    
    if input.is_empty() {
        return Command::Unknown("Empty command".to_string());
    }
    
    let parts: Vec<&str> = input.split_whitespace().collect();
    let command = parts[0].to_lowercase();
    
    match command.as_str() {
        "help" | "h" => Command::Help,
        "create" => {
            if parts.len() < 2 {
                Command::Unknown("Usage: create <username>".to_string())
            } else {
                Command::CreateSession(parts[1].to_string())
            }
        },
        "join" => {
            if parts.len() < 3 {
                Command::Unknown("Usage: join <session_link> <username>".to_string())
            } else {
                Command::JoinSession(parts[1].to_string(), parts[2].to_string())
            }
        },
        "leave" => Command::LeaveSession,
        "quit" | "exit" | "q" => Command::Quit,
        "position" | "pos" => {
            if parts.len() < 4 {
                Command::Unknown("Usage: position <x> <y> <z>".to_string())
            } else {
                let x = parts[1].parse::<f32>().unwrap_or(0.0);
                let y = parts[2].parse::<f32>().unwrap_or(0.0);
                let z = parts[3].parse::<f32>().unwrap_or(0.0);
                Command::SetPosition(x, y, z)
            }
        },
        "mute" => {
            if parts.len() < 2 {
                Command::Unknown("Usage: mute <true|false>".to_string())
            } else {
                let mute = parts[1].parse::<bool>().unwrap_or(true);
                Command::Mute(mute)
            }
        },
        "volume" => {
            if parts.len() < 3 {
                Command::Unknown("Usage: volume <participant_id> <level>".to_string())
            } else {
                let id = parts[1].to_string();
                let level = parts[2].parse::<f32>().unwrap_or(1.0);
                Command::SetVolume(id, level)
            }
        },
        _ => Command::Unknown(format!("Unknown command: {}", command)),
    }
}

pub async fn execute_command(command: Command, session: &mut SessionManager) -> Result<bool> {
    match command {
        Command::Help => {
            println!("Available commands:");
            println!("  help                      - Show this help");
            println!("  create <username>         - Create a new session");
            println!("  join <link> <username>    - Join an existing session");
            println!("  leave                     - Leave the current session");
            println!("  position <x> <y> <z>      - Set your position in virtual space");
            println!("  mute <true|false>         - Mute or unmute yourself");
            println!("  volume <user> <level>     - Set volume for a participant");
            println!("  quit                      - Quit the application");
            Ok(false)
        },
        Command::CreateSession(username) => {
            let link = session.create_session(&username).await?;
            println!("Created session with link: {}", link);
            println!("Share this link with others to join your session.");
            Ok(false)
        },
        Command::JoinSession(link, username) => {
            session.join_session(&link, &username).await?;
            println!("Joined session successfully!");
            Ok(false)
        },
        Command::LeaveSession => {
            session.leave_session().await?;
            println!("Left session.");
            Ok(false)
        },
        Command::Quit => {
            // Quit the application
            Ok(true)
        },
        Command::SetPosition(x, y, z) => {
            if let Some(local) = session.get_local_participant() {
                session.update_participant_position(&local.id, (x, y, z))?;
                println!("Position set to ({}, {}, {})", x, y, z);
            }
            Ok(false)
        },
        Command::Mute(mute) => {
            if let Some(local) = session.get_local_participant() {
                session.set_participant_muted(&local.id, mute)?;
                println!("{}", if mute { "Muted" } else { "Unmuted" });
            }
            Ok(false)
        },
        Command::SetVolume(id, volume) => {
            // Would need to implement volume control
            println!("Set volume for {} to {}", id, volume);
            Ok(false)
        },
        Command::Unknown(msg) => {
            println!("Error: {}", msg);
            Ok(false)
        },
    }
}