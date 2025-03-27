use std::str::FromStr;
use std::fmt;

/// Audio quality settings for the application
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioQuality {
    Low,
    Medium,
    High,
}

/// Main configuration struct for the application
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub audio_quality: AudioQuality,
    pub username: String,
    pub input_device: Option<String>,
    pub output_device: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            audio_quality: AudioQuality::Medium,
            username: "User".to_string(),
            input_device: None,
            output_device: None,
        }
    }
}

impl Config {
    /// Serializes the configuration to a string
    pub fn to_string(&self) -> String {
        let input_device = self.input_device.as_deref().unwrap_or("none");
        let output_device = self.output_device.as_deref().unwrap_or("none");
        
        format!(
            "audio_quality={:?}\nusername={}\ninput_device={}\noutput_device={}", 
            self.audio_quality, 
            self.username,
            input_device,
            output_device
        )
    }
}

// Custom error for configuration parsing
#[derive(Debug)]
pub struct ConfigParseError {
    message: String,
}

impl fmt::Display for ConfigParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Configuration error: {}", self.message)
    }
}

impl std::error::Error for ConfigParseError {}

impl FromStr for Config {
    type Err = ConfigParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut config = Config::default();
        
        for line in s.lines() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            
            let parts: Vec<&str> = line.splitn(2, '=').collect();
            if parts.len() != 2 {
                return Err(ConfigParseError { 
                    message: format!("Invalid line format: {}", line) 
                });
            }
            
            let key = parts[0].trim();
            let value = parts[1].trim();
            
            match key {
                "audio_quality" => {
                    config.audio_quality = match value {
                        "Low" => AudioQuality::Low,
                        "Medium" => AudioQuality::Medium,
                        "High" => AudioQuality::High,
                        _ => return Err(ConfigParseError {
                            message: format!("Unknown audio quality: {}", value)
                        }),
                    };
                },
                "username" => config.username = value.to_string(),
                "input_device" => {
                    config.input_device = if value == "none" { None } else { Some(value.to_string()) };
                },
                "output_device" => {
                    config.output_device = if value == "none" { None } else { Some(value.to_string()) };
                },
                _ => return Err(ConfigParseError {
                    message: format!("Unknown configuration key: {}", key)
                }),
            }
        }
        
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.audio_quality, AudioQuality::Medium);
    }
    
    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let serialized = config.to_string();
        let deserialized = Config::from_str(&serialized).unwrap();
        assert_eq!(config, deserialized);
    }
    
    #[test]
    fn test_custom_config() {
        let mut config = Config::default();
        config.audio_quality = AudioQuality::High;
        config.username = "TestUser".to_string();
        config.input_device = Some("Microphone".to_string());
        
        let serialized = config.to_string();
        let deserialized = Config::from_str(&serialized).unwrap();
        assert_eq!(config, deserialized);
    }
}