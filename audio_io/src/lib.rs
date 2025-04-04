//! Audio input/output handling for room.rs
//!
//! This crate interfaces with audio hardware using cpal.

use anyhow::anyhow;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BuildStreamError, Device, SampleFormat, Stream, StreamConfig};
use log::{debug, error, info, trace, warn};
use room_core::{AudioBuffer, Error, CHANNELS, SAMPLE_RATE};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Audio device interface.
pub struct AudioDevice {
    /// The host's default input device
    input_device: Device,
    /// The host's default output device
    output_device: Device,
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
            input_device,
            output_device,
            input_stream: None,
            output_stream: None,
            capture_sender: None,
            playback_receiver: None,
            playback_buffer: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// Start audio capture, sending captured samples to the provided channel
    pub fn start_capture(&mut self, sender: mpsc::Sender<AudioBuffer>) -> Result<(), Error> {
        // Store the sender
        self.capture_sender = Some(sender);

        // Configure the input stream
        let config = StreamConfig {
            channels: CHANNELS,
            sample_rate: cpal::SampleRate(SAMPLE_RATE),
            buffer_size: cpal::BufferSize::Default,
        };

        // Check what sample format is supported
        let supported_formats = self
            .input_device
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
            self.input_device.build_input_stream(
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
            self.input_device.build_input_stream(
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

    /// Start audio playback, receiving samples from the provided channel
    pub fn start_playback(&mut self, receiver: mpsc::Receiver<AudioBuffer>) -> Result<(), Error> {
        // Store the receiver
        self.playback_receiver = Some(receiver);

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
            self.output_device.build_output_stream(
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
            // Build the stream with i16 samples (converted from f32)
            self.output_device.build_output_stream(
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
                                    // Convert f32 to i16 and copy to both L and R channels
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
                            // Direct copy for stereo, with f32 to i16 conversion
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

    /// Stop audio capture
    pub fn stop_capture(&mut self) {
        if let Some(stream) = self.input_stream.take() {
            drop(stream);
            info!("Audio capture stopped");
        }
    }

    /// Stop audio playback
    pub fn stop_playback(&mut self) {
        if let Some(stream) = self.output_stream.take() {
            drop(stream);
            info!("Audio playback stopped");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_device_creation() {
        // Skip test if running in CI or headless environment
        if std::env::var("CI").is_ok() || std::env::var("DISPLAY").is_err() {
            return;
        }

        let device = AudioDevice::new();
        assert!(
            device.is_ok(),
            "Failed to create audio device: {:?}",
            device.err()
        );
    }
}
