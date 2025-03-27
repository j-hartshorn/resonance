// Session management module
// Manages audio communication sessions and participants

use std::collections::HashMap;
use anyhow::Result;
use crate::app::config::Config;

/// Represents a participant in the audio session
#[derive(Debug, Clone)]
pub struct Participant {
    pub id: String,
    pub name: String,
    pub position: (f32, f32, f32), // x, y, z coordinates in virtual space
    pub is_speaking: bool,
    pub volume: f32,
    pub muted: bool,
}

/// Manages the current audio communication session
pub struct SessionManager {
    config: Config,
    participants: HashMap<String, Participant>,
    local_id: String,
    session_link: Option<String>,
    connected: bool,
}

impl SessionManager {
    pub fn new(config: &Config) -> Self {
        // Generate a unique local ID
        let local_id = format!("user-{}", uuid::Uuid::new_v4().to_simple());
        
        Self {
            config: config.clone(),
            participants: HashMap::new(),
            local_id,
            session_link: None,
            connected: false,
        }
    }
    
    /// Create a new session and generate a shareable link
    pub async fn create_session(&mut self, name: &str) -> Result<String> {
        // Add local participant
        let local_participant = Participant {
            id: self.local_id.clone(),
            name: name.to_string(),
            position: (0.0, 0.0, 0.0),
            is_speaking: false,
            volume: 1.0,
            muted: false,
        };
        
        self.participants.insert(self.local_id.clone(), local_participant);
        
        // Generate session link (in a real implementation, this would involve the signaling service)
        let session_id = uuid::Uuid::new_v4().to_simple().to_string();
        let link = format!("resonance://{}", session_id);
        self.session_link = Some(link.clone());
        self.connected = true;
        
        Ok(link)
    }
    
    /// Join an existing session using a link
    pub async fn join_session(&mut self, link: &str, name: &str) -> Result<()> {
        // Parse the session ID from the link
        let session_id = link.strip_prefix("resonance://")
            .ok_or_else(|| anyhow::anyhow!("Invalid session link format"))?;
        
        // Add local participant
        let local_participant = Participant {
            id: self.local_id.clone(),
            name: name.to_string(),
            position: (0.0, 0.0, 0.0),
            is_speaking: false,
            volume: 1.0,
            muted: false,
        };
        
        self.participants.insert(self.local_id.clone(), local_participant);
        self.session_link = Some(link.to_string());
        self.connected = true;
        
        // In a real implementation, we would connect to the session here
        
        Ok(())
    }
    
    /// Update session state (call periodically)
    pub async fn update(&mut self) -> Result<()> {
        // In a real implementation, this would:
        // - Update participant speaking status
        // - Handle participants joining/leaving
        // - Update network statistics
        // - etc.
        
        Ok(())
    }
    
    /// Get all current participants
    pub fn get_participants(&self) -> Vec<&Participant> {
        self.participants.values().collect()
    }
    
    /// Get the local participant
    pub fn get_local_participant(&self) -> Option<&Participant> {
        self.participants.get(&self.local_id)
    }
    
    /// Check if connected to a session
    pub fn is_connected(&self) -> bool {
        self.connected
    }
    
    /// Leave the current session
    pub async fn leave_session(&mut self) -> Result<()> {
        self.participants.clear();
        self.session_link = None;
        self.connected = false;
        
        Ok(())
    }
    
    /// Update the position of a participant in the virtual space
    pub fn update_participant_position(&mut self, id: &str, position: (f32, f32, f32)) -> Result<()> {
        if let Some(participant) = self.participants.get_mut(id) {
            participant.position = position;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Participant not found"))
        }
    }
    
    /// Mute or unmute a participant
    pub fn set_participant_muted(&mut self, id: &str, muted: bool) -> Result<()> {
        if let Some(participant) = self.participants.get_mut(id) {
            participant.muted = muted;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Participant not found"))
        }
    }
}