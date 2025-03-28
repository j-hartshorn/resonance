use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone)]
pub struct Command {
    pub name: String,
    pub args: Vec<String>,
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "/{}", self.name)?;
        for arg in &self.args {
            write!(f, " {}", arg)?;
        }
        Ok(())
    }
}

pub trait CommandHandler {
    fn join_session(&mut self, link: &str) -> Result<(), String>;
    fn create_session(&mut self) -> Result<String, String>;
    fn set_volume(&mut self, level: u8) -> Result<(), String>;
    fn set_position(&mut self, x: f32, y: f32, z: f32) -> Result<(), String>;
    fn list_participants(&self) -> Result<Vec<String>, String>;
    fn help(&self) -> Vec<String>;
}

pub struct CommandProcessor {
    commands: HashMap<String, String>,
}

impl CommandProcessor {
    pub fn new() -> Self {
        let mut commands = HashMap::new();
        commands.insert(
            "join".to_string(),
            "Join a session using a connection link".to_string(),
        );
        commands.insert("create".to_string(), "Create a new session".to_string());
        commands.insert("volume".to_string(), "Set volume level (0-100)".to_string());
        commands.insert(
            "position".to_string(),
            "Set position in virtual space (x y z)".to_string(),
        );
        commands.insert(
            "who".to_string(),
            "List participants in the session".to_string(),
        );
        commands.insert("help".to_string(), "Show available commands".to_string());
        commands.insert("quit".to_string(), "Exit the application".to_string());

        Self { commands }
    }

    pub fn parse(&self, input: &str) -> Result<Command, String> {
        let input = input.trim();

        if !input.starts_with('/') {
            return Err("Not a command (must start with /)".to_string());
        }

        let parts: Vec<&str> = input[1..].split_whitespace().collect();
        if parts.is_empty() {
            return Err("Empty command".to_string());
        }

        let name = parts[0].to_string();
        let args = parts[1..].iter().map(|s| s.to_string()).collect();

        Ok(Command { name, args })
    }

    pub fn execute<H: CommandHandler>(
        &self,
        input: &str,
        handler: &mut H,
    ) -> Result<String, String> {
        let cmd = self.parse(input)?;

        match cmd.name.as_str() {
            "join" => {
                if cmd.args.is_empty() {
                    return Err("Missing session link".to_string());
                }
                handler.join_session(&cmd.args[0])?;
                Ok("Joined session successfully".to_string())
            }
            "create" => {
                let link = handler.create_session()?;
                Ok(format!("Session created. Share this link: {}", link))
            }
            "volume" => {
                if cmd.args.is_empty() {
                    return Err("Missing volume level".to_string());
                }
                let level: u8 = cmd.args[0]
                    .parse()
                    .map_err(|_| "Volume must be a number between 0-100".to_string())?;
                handler.set_volume(level)?;
                Ok(format!("Volume set to {}", level))
            }
            "position" => {
                if cmd.args.len() < 3 {
                    return Err("Missing position coordinates (need x y z)".to_string());
                }
                let x: f32 = cmd.args[0]
                    .parse()
                    .map_err(|_| "X coordinate must be a number".to_string())?;
                let y: f32 = cmd.args[1]
                    .parse()
                    .map_err(|_| "Y coordinate must be a number".to_string())?;
                let z: f32 = cmd.args[2]
                    .parse()
                    .map_err(|_| "Z coordinate must be a number".to_string())?;

                handler.set_position(x, y, z)?;
                Ok(format!("Position set to ({}, {}, {})", x, y, z))
            }
            "who" => {
                let participants = handler.list_participants()?;
                if participants.is_empty() {
                    return Ok("No participants in the session".to_string());
                }

                let mut result = String::from("Participants in this session:");
                for p in participants {
                    result.push_str(&format!("\n  - {}", p));
                }

                Ok(result)
            }
            "help" => {
                let commands = handler.help();
                let mut result = String::from("Available commands:");

                for cmd in commands {
                    result.push_str(&format!("\n  {}", cmd));
                }

                Ok(result)
            }
            "quit" => Ok("Quitting application...".to_string()),
            _ => Err(format!("Unknown command: /{}", cmd.name)),
        }
    }

    pub fn get_commands(&self) -> Vec<(String, String)> {
        self.commands
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockall::predicate::*;
    use mockall::*;

    mock! {
        AppState {}
        impl CommandHandler for AppState {
            fn join_session(&mut self, link: &str) -> Result<(), String>;
            fn create_session(&mut self) -> Result<String, String>;
            fn set_volume(&mut self, level: u8) -> Result<(), String>;
            fn set_position(&mut self, x: f32, y: f32, z: f32) -> Result<(), String>;
            fn list_participants(&self) -> Result<Vec<String>, String>;
            fn help(&self) -> Vec<String>;
        }
    }

    #[test]
    fn test_command_parsing() {
        let processor = CommandProcessor::new();

        let cmd = processor.parse("/join abc123").unwrap();
        assert_eq!(cmd.name, "join");
        assert_eq!(cmd.args, vec!["abc123"]);

        let cmd = processor.parse("/volume 80").unwrap();
        assert_eq!(cmd.name, "volume");
        assert_eq!(cmd.args, vec!["80"]);

        // Test error case
        let result = processor.parse("not a command");
        assert!(result.is_err());
    }

    #[test]
    fn test_command_execution() {
        let processor = CommandProcessor::new();
        let mut app_state = MockAppState::new();

        // Setup expectations
        app_state
            .expect_join_session()
            .with(eq("abc123"))
            .times(1)
            .returning(|_| Ok(()));

        app_state
            .expect_create_session()
            .times(1)
            .returning(|| Ok("https://link.example".to_string()));

        app_state
            .expect_set_volume()
            .with(eq(80))
            .times(1)
            .returning(|_| Ok(()));

        // Test join command
        let result = processor.execute("/join abc123", &mut app_state);
        assert!(result.is_ok());

        // Test create command
        let result = processor.execute("/create", &mut app_state);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("https://link.example"));

        // Test volume command
        let result = processor.execute("/volume 80", &mut app_state);
        assert!(result.is_ok());
    }
}
