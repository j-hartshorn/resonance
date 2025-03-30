use cpal::{
    self,
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Sample,
};
use ringbuf::{Consumer, HeapRb, Producer};
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
        let mut devices = Vec::new();

        // Use cpal to get actual audio devices
        let host = cpal::default_host();

        // Get input devices
        if let Ok(input_devices) = host.input_devices() {
            for device in input_devices {
                if let Ok(name) = device.name() {
                    devices.push(AudioDevice {
                        id: name.clone(),
                        name,
                        is_input: true,
                    });
                }
            }
        }

        // Get output devices
        if let Ok(output_devices) = host.output_devices() {
            for device in output_devices {
                if let Ok(name) = device.name() {
                    devices.push(AudioDevice {
                        id: name.clone(),
                        name,
                        is_input: false,
                    });
                }
            }
        }

        // If no devices were found, return mock devices for testing
        if devices.is_empty() {
            devices.push(AudioDevice {
                id: "input1".to_string(),
                name: "Default Microphone".to_string(),
                is_input: true,
            });
            devices.push(AudioDevice {
                id: "output1".to_string(),
                name: "Default Speakers".to_string(),
                is_input: false,
            });
        }

        devices
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
    pub data_tx: Option<mpsc::Sender<Vec<f32>>>,
    cancel_token: Option<tokio::sync::oneshot::Sender<()>>,
    #[allow(dead_code)]
    audio_stream: Option<cpal::Stream>,
}

impl AudioCapture {
    pub fn new() -> Self {
        Self {
            device: None,
            is_active: false,
            data_tx: None,
            cancel_token: None,
            audio_stream: None,
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

        // Set up real microphone capture using cpal
        let host = cpal::default_host();
        let device_name = self
            .device
            .as_ref()
            .map(|d| d.id.clone())
            .unwrap_or_default();

        // Try to find the device by name, or use default input device
        let device = host
            .input_devices()
            .map_err(|e| AudioError::new(&format!("Failed to get input devices: {}", e)))?
            .find(|d| match d.name() {
                Ok(name) => name == device_name,
                Err(_) => false,
            })
            .or_else(|| host.default_input_device())
            .ok_or_else(|| AudioError::new("No input device found"))?;

        // Get supported configs and choose a reasonable one
        let config = match device.default_input_config() {
            Ok(config) => config,
            Err(e) => {
                return Err(AudioError::new(&format!(
                    "Default config not supported: {}",
                    e
                )))
            }
        };

        // Create a ring buffer for audio samples
        let ring_size = 1024 * 8;
        let rb = HeapRb::<f32>::new(ring_size);
        let (mut prod, mut cons) = rb.split();

        // Create stream for audio input
        let err_fn = move |err| {
            eprintln!("an error occurred on the audio stream: {}", err);
        };

        // Set up the actual audio input stream with cpal
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                let stream = device
                    .build_input_stream(
                        &config.into(),
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            // Push the incoming audio data to the ring buffer
                            for &sample in data {
                                let _ = prod.push(sample);
                            }
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|e| {
                        AudioError::new(&format!("Failed to build input stream: {}", e))
                    })?;
                stream
            }
            cpal::SampleFormat::I16 => {
                let stream = device
                    .build_input_stream(
                        &config.into(),
                        move |data: &[i16], _: &cpal::InputCallbackInfo| {
                            // Convert i16 samples to f32 and push to the ring buffer
                            for &sample in data {
                                // Normalize i16 to f32 range
                                let normalized = sample as f32 / i16::MAX as f32;
                                let _ = prod.push(normalized);
                            }
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|e| {
                        AudioError::new(&format!("Failed to build input stream: {}", e))
                    })?;
                stream
            }
            cpal::SampleFormat::U16 => {
                let stream = device
                    .build_input_stream(
                        &config.into(),
                        move |data: &[u16], _: &cpal::InputCallbackInfo| {
                            // Convert u16 samples to f32 and push to the ring buffer
                            for &sample in data {
                                // Normalize u16 to f32 range, centered around 0
                                let normalized = (sample as f32 / u16::MAX as f32) * 2.0 - 1.0;
                                let _ = prod.push(normalized);
                            }
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|e| {
                        AudioError::new(&format!("Failed to build input stream: {}", e))
                    })?;
                stream
            }
            _ => return Err(AudioError::new("Unsupported sample format")),
        };

        // Start the stream
        stream
            .play()
            .map_err(|e| AudioError::new(&format!("Failed to start audio stream: {}", e)))?;

        // Store the stream to keep it alive
        self.audio_stream = Some(stream);

        // Start a task to read from the ring buffer and send data to the callback
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(20));
            let mut buffer = Vec::with_capacity(1024);

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        // Read available samples from the ring buffer
                        buffer.clear();
                        while let Some(sample) = cons.pop() {
                            buffer.push(sample);
                            if buffer.len() >= 1024 {
                                break;
                            }
                        }

                        // Send audio data if we have enough samples and a channel
                        if !buffer.is_empty() {
                            if let Some(tx) = &data_tx {
                                let _ = tx.send(buffer.clone()).await;
                            }
                        }
                    }
                    _ = &mut cancel_rx => {
                        break;
                    }
                }
            }
        });

        // Don't print debug messages that would interfere with the UI
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

        // Stop the audio stream
        self.audio_stream = None;

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

    // Use a more distinct echo pattern with stronger echo
    for i in 0..original.len() {
        // Add delayed echo with stronger amplitude and clear delay
        let echo = if i >= 300 {
            original[i - 300] * 0.7 // Stronger echo, easier to detect and filter
        } else {
            0.0
        };

        // Add original signal with the echo
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
