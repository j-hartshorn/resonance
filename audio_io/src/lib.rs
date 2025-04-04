//! Audio input/output handling for room.rs
//!
//! This crate interfaces with audio hardware using cpal.

use anyhow::anyhow;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BuildStreamError, Device, SampleFormat, Stream, StreamConfig};
use log::{debug, error, info, trace, warn};
use rand::Rng;
use room_core::{AudioBuffer, Error, CHANNELS, SAMPLE_RATE};
use std::f32::consts::PI;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Audio device interface.
pub struct AudioDevice {
    /// The host's default input device
    input_device: Option<Device>,
    /// The host's default output device
    output_device: Option<Device>,
    /// The input stream (capture)
    input_stream: Option<Stream>,
    /// The output stream (playback)
    output_stream: Option<Stream>,
    /// Channel to send captured audio samples
    capture_sender: Option<mpsc::Sender<AudioBuffer>>,
    /// Channel to receive audio samples for playback
    playback_receiver: Option<mpsc::Receiver<AudioBuffer>>,
    /// Ring buffer for audio playback
    playback_buffer: Arc<Mutex<AudioBuffer>>,
    /// In test mode - generates artificial sound instead of using real microphone
    test_mode: bool,
    /// Tone generator state for test mode
    test_tone_state: TestToneState,
}

/// State for generating test tones
#[derive(Clone)]
struct TestToneState {
    /// Current phase of the sine wave
    phase: f32,
    /// Frequency of the tone in Hz
    frequency: f32,
    /// Last time the test tone was generated
    last_update: Instant,
}

impl Default for TestToneState {
    fn default() -> Self {
        Self {
            phase: 0.0,
            frequency: 440.0, // A4 note
            last_update: Instant::now(),
        }
    }
}

impl AudioDevice {
    /// Initialize the audio device with default settings
    pub fn new() -> Result<Self, Error> {
        let host = cpal::default_host();

        // Get the default input device
        let input_device = host
            .default_input_device()
            .ok_or_else(|| Error::Audio("No default input device found".to_string()))?;

        // Get the default output device
        let output_device = host
            .default_output_device()
            .ok_or_else(|| Error::Audio("No default output device found".to_string()))?;

        info!(
            "Using input device: {}",
            input_device.name().unwrap_or_default()
        );
        info!(
            "Using output device: {}",
            output_device.name().unwrap_or_default()
        );

        Ok(Self {
            input_device: Some(input_device),
            output_device: Some(output_device),
            input_stream: None,
            output_stream: None,
            capture_sender: None,
            playback_receiver: None,
            playback_buffer: Arc::new(Mutex::new(Vec::new())),
            test_mode: false,
            test_tone_state: TestToneState::default(),
        })
    }

    /// Create a new audio device in test mode
    pub fn new_test_mode() -> Self {
        info!("Creating audio device in test mode");
        Self {
            input_device: None,
            output_device: None,
            input_stream: None,
            output_stream: None,
            capture_sender: None,
            playback_receiver: None,
            playback_buffer: Arc::new(Mutex::new(Vec::new())),
            test_mode: true,
            test_tone_state: TestToneState::default(),
        }
    }

    /// Start audio capture, sending captured samples to the provided channel
    pub fn start_capture(&mut self, sender: mpsc::Sender<AudioBuffer>) -> Result<(), Error> {
        // Store the sender
        self.capture_sender = Some(sender);

        if self.test_mode {
            return self.start_test_capture();
        }

        // Configure the input stream
        let config = StreamConfig {
            channels: CHANNELS,
            sample_rate: cpal::SampleRate(SAMPLE_RATE),
            buffer_size: cpal::BufferSize::Default,
        };

        // Check what sample format is supported
        let supported_formats = self
            .input_device
            .as_ref()
            .unwrap()
            .supported_input_configs()
            .map_err(|e| Error::Audio(format!("Error querying supported formats: {}", e)))?;

        let mut supports_f32 = false;
        for format in supported_formats {
            if format.sample_format() == SampleFormat::F32 {
                supports_f32 = true;
                break;
            }
        }

        // Create the input stream
        let err_fn = move |err| {
            error!("An error occurred on the input audio stream: {}", err);
        };

        let sender = self.capture_sender.as_ref().unwrap().clone();

        let stream = if supports_f32 {
            // Build the stream with f32 samples
            self.input_device.as_ref().unwrap().build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    // Clone the data to an AudioBuffer and send it
                    let buffer = AudioBuffer::from(data.to_vec());
                    if let Err(e) = sender.try_send(buffer) {
                        match e {
                            tokio::sync::mpsc::error::TrySendError::Full(_) => {
                                // This is normal if the receiver isn't keeping up
                                trace!("Capture channel full, dropping buffer");
                            }
                            _ => {
                                error!("Failed to send audio buffer: {}", e);
                            }
                        }
                    }
                },
                err_fn,
                None,
            )
        } else {
            // Build the stream with i16 samples (converted to f32)
            self.input_device.as_ref().unwrap().build_input_stream(
                &config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    // Convert i16 to f32 and send
                    let buffer = AudioBuffer::from(
                        data.iter()
                            .map(|&s| s as f32 / 32768.0)
                            .collect::<Vec<f32>>(),
                    );
                    if let Err(e) = sender.try_send(buffer) {
                        match e {
                            tokio::sync::mpsc::error::TrySendError::Full(_) => {
                                trace!("Capture channel full, dropping buffer");
                            }
                            _ => {
                                error!("Failed to send audio buffer: {}", e);
                            }
                        }
                    }
                },
                err_fn,
                None,
            )
        }
        .map_err(|e| Error::Audio(format!("Failed to build input stream: {}", e)))?;

        // Start the stream
        stream
            .play()
            .map_err(|e| Error::Audio(format!("Failed to start input stream: {}", e)))?;

        // Store the stream
        self.input_stream = Some(stream);

        info!("Audio capture started");
        Ok(())
    }

    /// Start test mode audio capture - generates test tones instead of using microphone
    fn start_test_capture(&mut self) -> Result<(), Error> {
        let sender = self.capture_sender.as_ref().unwrap().clone();
        let test_tone_state = Arc::new(Mutex::new(self.test_tone_state.clone()));

        // Spawn a task to generate test audio data
        tokio::spawn(async move {
            let sample_rate = SAMPLE_RATE as f32;
            let buffer_size = 1024; // Reasonable buffer size
            let buffer_duration = Duration::from_secs_f32(buffer_size as f32 / sample_rate);

            loop {
                let mut buffer = Vec::with_capacity(buffer_size);

                {
                    let mut state = test_tone_state.lock().unwrap();
                    let elapsed = state.last_update.elapsed().as_secs_f32();
                    state.last_update = Instant::now();

                    // Advance the phase based on elapsed time
                    state.phase += state.frequency * elapsed * 2.0 * PI;
                    if state.phase > 2.0 * PI {
                        state.phase -= 2.0 * PI;
                    }

                    // Generate sine wave samples
                    for i in 0..buffer_size {
                        let sample_phase =
                            state.phase + (i as f32 / sample_rate) * state.frequency * 2.0 * PI;
                        let sample = (sample_phase.sin() * 0.2) as f32; // 0.2 = 20% amplitude
                        buffer.push(sample);
                    }
                }

                if let Err(e) = sender.send(buffer).await {
                    error!("Failed to send test audio buffer: {}", e);
                    break;
                }

                // Sleep to simulate real-time audio capture
                tokio::time::sleep(buffer_duration / 2).await;
            }
        });

        info!("Test mode audio capture started");
        Ok(())
    }

    /// Start audio playback, receiving samples from the provided channel
    pub fn start_playback(&mut self, receiver: mpsc::Receiver<AudioBuffer>) -> Result<(), Error> {
        // Store the receiver
        self.playback_receiver = Some(receiver);

        if self.test_mode {
            return self.start_test_playback();
        }

        // Configure the output stream
        let config = StreamConfig {
            channels: 2, // Always use stereo for output
            sample_rate: cpal::SampleRate(SAMPLE_RATE),
            buffer_size: cpal::BufferSize::Default,
        };

        // Start a background task to receive buffers and update the playback buffer
        let playback_buffer = self.playback_buffer.clone();
        let mut receiver = self.playback_receiver.take().unwrap();
        tokio::spawn(async move {
            while let Some(buffer) = receiver.recv().await {
                let mut pb = playback_buffer.lock().unwrap();
                *pb = buffer;
            }
        });

        // Check what sample format is supported
        let supported_formats = self
            .output_device
            .as_ref()
            .unwrap()
            .supported_output_configs()
            .map_err(|e| Error::Audio(format!("Error querying supported formats: {}", e)))?;

        let mut supports_f32 = false;
        for format in supported_formats {
            if format.sample_format() == SampleFormat::F32 {
                supports_f32 = true;
                break;
            }
        }

        // Create the output stream
        let err_fn = move |err| {
            error!("An error occurred on the output audio stream: {}", err);
        };

        let playback_buffer = self.playback_buffer.clone();

        let stream = if supports_f32 {
            // Build the stream with f32 samples
            self.output_device.as_ref().unwrap().build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    // Fill the output buffer with data from our playback buffer
                    let pb = playback_buffer.lock().unwrap();

                    // If we have audio data, copy it to output buffer
                    // Otherwise, fill with silence (zeros)
                    if pb.is_empty() {
                        for sample in data.iter_mut() {
                            *sample = 0.0;
                        }
                    } else {
                        // Duplicate mono to stereo if needed
                        if CHANNELS == 1 {
                            let mut src_idx = 0;
                            for chunk in data.chunks_mut(2) {
                                if src_idx < pb.len() {
                                    // Copy the same mono sample to both L and R channels
                                    let sample = pb[src_idx];
                                    for output in chunk.iter_mut() {
                                        *output = sample;
                                    }
                                    src_idx += 1;
                                } else {
                                    // Fill remaining with silence
                                    for output in chunk.iter_mut() {
                                        *output = 0.0;
                                    }
                                }
                            }
                        } else {
                            // Direct copy for stereo
                            let len = std::cmp::min(data.len(), pb.len());
                            data[..len].copy_from_slice(&pb[..len]);

                            // Fill remaining with silence
                            for i in len..data.len() {
                                data[i] = 0.0;
                            }
                        }
                    }
                },
                err_fn,
                None,
            )
        } else {
            // Build the stream with i16 samples (convert from f32)
            self.output_device.as_ref().unwrap().build_output_stream(
                &config,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    // Fill the output buffer with data from our playback buffer
                    let pb = playback_buffer.lock().unwrap();

                    // If we have audio data, copy it to output buffer
                    // Otherwise, fill with silence (zeros)
                    if pb.is_empty() {
                        for sample in data.iter_mut() {
                            *sample = 0;
                        }
                    } else {
                        // Duplicate mono to stereo if needed
                        if CHANNELS == 1 {
                            let mut src_idx = 0;
                            for chunk in data.chunks_mut(2) {
                                if src_idx < pb.len() {
                                    // Convert f32 to i16 and copy the same mono sample to both L and R channels
                                    let sample = (pb[src_idx] * 32767.0) as i16;
                                    for output in chunk.iter_mut() {
                                        *output = sample;
                                    }
                                    src_idx += 1;
                                } else {
                                    // Fill remaining with silence
                                    for output in chunk.iter_mut() {
                                        *output = 0;
                                    }
                                }
                            }
                        } else {
                            // Convert f32 to i16 for each sample
                            let len = std::cmp::min(data.len(), pb.len());
                            for i in 0..len {
                                data[i] = (pb[i] * 32767.0) as i16;
                            }

                            // Fill remaining with silence
                            for i in len..data.len() {
                                data[i] = 0;
                            }
                        }
                    }
                },
                err_fn,
                None,
            )
        }
        .map_err(|e| Error::Audio(format!("Failed to build output stream: {}", e)))?;

        // Start the stream
        stream
            .play()
            .map_err(|e| Error::Audio(format!("Failed to start output stream: {}", e)))?;

        // Store the stream
        self.output_stream = Some(stream);

        info!("Audio playback started");
        Ok(())
    }

    /// Start test mode audio playback - just consumes the buffers
    fn start_test_playback(&mut self) -> Result<(), Error> {
        // Start a background task to simply receive and log audio buffers
        let mut receiver = self.playback_receiver.take().unwrap();
        tokio::spawn(async move {
            let mut log_counter = 0;
            while let Some(buffer) = receiver.recv().await {
                // Log only occasionally (every 100th buffer) to avoid flooding logs
                log_counter += 1;
                if log_counter >= 100 {
                    debug!("Test playback: received buffer of {} samples", buffer.len());
                    log_counter = 0;
                }
            }
        });

        info!("Test mode audio playback started");
        Ok(())
    }

    /// Stop audio capture
    pub fn stop_capture(&mut self) {
        self.input_stream = None;
        self.capture_sender = None;
    }

    /// Stop audio playback
    pub fn stop_playback(&mut self) {
        self.output_stream = None;
        self.playback_receiver = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_device_creation() {
        // Skip actual device creation in CI environments
        if std::env::var("CI").is_ok() {
            return;
        }

        // This might fail if no audio devices are available
        let device = AudioDevice::new();
        if device.is_err() {
            println!(
                "Skipping test, no audio device available: {:?}",
                device.err()
            );
            return;
        }
    }

    #[test]
    fn test_test_mode_creation() {
        let device = AudioDevice::new_test_mode();
        assert!(device.test_mode);
        assert!(device.input_device.is_none());
        assert!(device.output_device.is_none());
    }
}
