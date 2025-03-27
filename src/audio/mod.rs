// Audio processing module
// Handles audio capture, processing, and playback

pub mod capture;
pub mod spatial;
pub mod voice;

use anyhow::Result;
use crate::app::config::Config;
use crate::audio::capture::AudioSystem;

/// Set up the audio subsystem
pub fn setup(config: &Config) -> Result<AudioSystem> {
    // Initialize audio system
    let audio_system = AudioSystem::new(config)?;
    
    // Initialize voice processor
    voice::init(config)?;
    
    // Initialize spatial audio processor
    spatial::init(config)?;
    
    Ok(audio_system)
}