use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

// Define the required types
#[derive(Debug, Clone)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_input: bool,
}

impl fmt::Display for AudioDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({})",
            self.name,
            if self.is_input { "Input" } else { "Output" }
        )
    }
}

#[derive(Debug)]
pub struct AudioError {
    message: String,
}

impl AudioError {
    fn new(message: &str) -> Self {
        Self {
            message: message.to_string(),
        }
    }
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Audio Error: {}", self.message)
    }
}

impl Error for AudioError {}

pub struct AudioDeviceManager {
    current_input_device: Option<AudioDevice>,
    current_output_device: Option<AudioDevice>,
}

impl AudioDeviceManager {
    pub fn new() -> Self {
        Self {
            current_input_device: None,
            current_output_device: None,
        }
    }

    pub fn enumerate_devices() -> Vec<AudioDevice> {
        // In a real implementation, this would use cpal or similar to get actual devices
        // For testing purposes, we'll return mock devices
        vec![
            AudioDevice {
                id: "input1".to_string(),
                name: "Default Microphone".to_string(),
                is_input: true,
            },
            AudioDevice {
                id: "output1".to_string(),
                name: "Default Speakers".to_string(),
                is_input: false,
            },
        ]
    }

    pub fn select_input_device(&mut self, device: &AudioDevice) -> Result<(), AudioError> {
        if !device.is_input {
            return Err(AudioError::new(
                "Attempted to select an output device as input",
            ));
        }

        self.current_input_device = Some(device.clone());
        Ok(())
    }

    pub fn select_output_device(&mut self, device: &AudioDevice) -> Result<(), AudioError> {
        if device.is_input {
            return Err(AudioError::new(
                "Attempted to select an input device as output",
            ));
        }

        self.current_output_device = Some(device.clone());
        Ok(())
    }

    pub fn current_input_device(&self) -> Option<&AudioDevice> {
        self.current_input_device.as_ref()
    }

    pub fn current_output_device(&self) -> Option<&AudioDevice> {
        self.current_output_device.as_ref()
    }
}

// Define AudioCapture struct
pub struct AudioCapture {
    device: Option<AudioDevice>,
    is_active: bool,
    data_tx: Option<mpsc::Sender<Vec<f32>>>,
    cancel_token: Option<tokio::sync::oneshot::Sender<()>>,
}

impl AudioCapture {
    pub fn new() -> Self {
        Self {
            device: None,
            is_active: false,
            data_tx: None,
            cancel_token: None,
        }
    }

    pub fn set_device(&mut self, device: AudioDevice) -> Result<(), AudioError> {
        if !device.is_input {
            return Err(AudioError::new("Cannot capture from output device"));
        }
        self.device = Some(device);
        Ok(())
    }

    pub fn set_data_callback<F>(&mut self, callback: F)
    where
        F: Fn(Vec<f32>) + Send + Sync + 'static,
    {
        // Create a channel for passing audio data
        let (tx, mut rx) = mpsc::channel::<Vec<f32>>(100);
        self.data_tx = Some(tx);

        // Spawn a task to listen for data and call the callback
        let callback = Arc::new(callback);
        tokio::spawn(async move {
            while let Some(data) = rx.recv().await {
                callback(data);
            }
        });
    }

    pub async fn start(&mut self) -> Result<(), AudioError> {
        if self.is_active {
            return Err(AudioError::new("Audio capture already started"));
        }

        // Ensure we have a device
        if self.device.is_none() {
            // If no device is set, use the default
            let devices = AudioDeviceManager::enumerate_devices();
            let default_input = devices.iter().find(|d| d.is_input).cloned();

            if let Some(device) = default_input {
                self.device = Some(device);
            } else {
                return Err(AudioError::new("No input device available"));
            }
        }

        // Create a cancel channel
        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel();
        self.cancel_token = Some(cancel_tx);

        // Clone the data tx for the capture task
        let data_tx = self.data_tx.clone();

        // Start the capture task
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(20));

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        // Generate dummy audio data (in real implementation, this would be from the device)
                        let audio_data = generate_test_audio();

                        // Send it to our callback handler if one exists
                        if let Some(tx) = &data_tx {
                            let _ = tx.send(audio_data).await;
                        }
                    }
                    _ = &mut cancel_rx => {
                        break;
                    }
                }
            }
        });

        self.is_active = true;
        Ok(())
    }

    pub async fn stop(&mut self) -> Result<(), AudioError> {
        if !self.is_active {
            return Err(AudioError::new("Audio capture not started"));
        }

        // Cancel the capture task
        if let Some(cancel_token) = self.cancel_token.take() {
            let _ = cancel_token.send(());
        }

        self.is_active = false;
        Ok(())
    }
}

// Helper function to generate test audio data
pub fn generate_test_audio() -> Vec<f32> {
    // Generate 1024 samples of a simple sine wave
    let mut data = Vec::with_capacity(1024);
    for i in 0..1024 {
        let sample = (i as f32 * 0.01).sin() * 0.5;
        data.push(sample);
    }
    data
}

pub fn generate_test_mono_audio() -> Vec<f32> {
    generate_test_audio()
}

pub fn generate_test_audio_with_echo() -> Vec<f32> {
    let original = generate_test_audio();
    let mut with_echo = Vec::with_capacity(original.len());

    for i in 0..original.len() {
        let echo = if i >= 200 {
            original[i - 200] * 0.5
        } else {
            0.0
        };
        with_echo.push(original[i] + echo);
    }

    with_echo
}

pub fn generate_test_silence() -> Vec<f32> {
    vec![0.0; 1024]
}

pub fn generate_test_speech() -> Vec<f32> {
    // For test purposes, we'll just create a higher amplitude signal
    let mut speech = generate_test_audio();
    for sample in &mut speech {
        *sample *= 3.0;
    }
    speech
}

pub fn measure_echo_level(audio: &[f32]) -> f32 {
    // Simple metric for echo: standard deviation of samples
    let mean = audio.iter().sum::<f32>() / audio.len() as f32;
    let variance = audio.iter().map(|&s| (s - mean).powi(2)).sum::<f32>() / audio.len() as f32;
    variance.sqrt()
}

// Tests for AudioDeviceManager
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_devices_enumeration() {
        let devices = AudioDeviceManager::enumerate_devices();
        assert!(!devices.is_empty());
    }

    #[test]
    fn test_audio_device_selection() {
        let mut manager = AudioDeviceManager::new();
        let devices = AudioDeviceManager::enumerate_devices();
        if !devices.is_empty() {
            // Find an input device
            let input_device = devices.iter().find(|d| d.is_input).unwrap();
            let result = manager.select_input_device(input_device);
            assert!(result.is_ok());
        }
    }

    #[tokio::test]
    async fn test_audio_capture_start_stop() {
        let mut capture = AudioCapture::new();
        assert!(capture.start().await.is_ok());
        assert!(capture.stop().await.is_ok());
    }

    #[tokio::test]
    async fn test_audio_data_received() {
        let mut capture = AudioCapture::new();
        let (tx, mut rx) = tokio::sync::mpsc::channel(100);

        capture.set_data_callback(move |data| {
            let _ = tx.try_send(data.clone());
        });

        capture.start().await.unwrap();

        // Should receive audio data within 1 second
        tokio::select! {
            data = rx.recv() => {
                assert!(data.is_some());
                assert!(!data.unwrap().is_empty());
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                panic!("Timed out waiting for audio data");
            }
        }

        capture.stop().await.unwrap();
    }
}
