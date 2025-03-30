use super::capture::{
    generate_test_audio, generate_test_audio_with_echo, generate_test_silence,
    generate_test_speech, measure_echo_level,
};
use std::sync::{Arc, Mutex};

// Voice processor for handling microphone audio
#[derive(Clone)]
pub struct VoiceProcessor {
    vad_threshold: f32,
    echo_cancellation_enabled: bool,
    muted: bool,
    // We'll store the far end buffer for echo cancellation
    far_end_buffer: Arc<Mutex<Vec<f32>>>,
}

impl VoiceProcessor {
    pub fn new() -> Self {
        Self {
            vad_threshold: 0.05, // Lower threshold for more sensitive voice detection
            echo_cancellation_enabled: true,
            muted: false,
            far_end_buffer: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn with_vad_threshold(mut self, threshold: f32) -> Self {
        self.vad_threshold = threshold;
        self
    }

    pub fn with_echo_cancellation(mut self, enabled: bool) -> Self {
        self.echo_cancellation_enabled = enabled;
        self
    }

    pub fn with_muted(mut self, muted: bool) -> Self {
        self.muted = muted;
        self
    }

    pub fn is_muted(&self) -> bool {
        self.muted
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    pub fn toggle_mute(&mut self) -> bool {
        self.muted = !self.muted;
        self.muted
    }

    pub fn process(&self, input: Vec<f32>) -> Vec<f32> {
        // If muted, return silence
        if self.muted {
            return vec![0.0; input.len()];
        }

        // Clone the input for processing
        let mut output = input.clone();

        // Apply echo cancellation if enabled
        if self.echo_cancellation_enabled {
            // Get the far end audio for reference
            let far_end = self.far_end_buffer.lock().unwrap().clone();

            if !far_end.is_empty() {
                output = self.apply_echo_cancellation(output, &far_end);
            }
        }

        output
    }

    // Set the far-end audio (what's coming from the speakers)
    pub fn set_far_end_audio(&mut self, audio: &[f32]) {
        let mut far_end = self.far_end_buffer.lock().unwrap();
        *far_end = audio.to_vec();
    }

    pub fn detect_voice_activity(&self, audio: &[f32]) -> bool {
        // Simple energy-based voice activity detection
        let energy = audio.iter().map(|&sample| sample.powi(2)).sum::<f32>() / audio.len() as f32;
        energy > self.vad_threshold
    }

    // Basic echo cancellation implementation
    // In a real implementation we would use the webrtc-audio-processing library
    // But for now we'll use a simpler approach that still works
    fn apply_echo_cancellation(&self, input: Vec<f32>, far_end: &[f32]) -> Vec<f32> {
        // Improved echo cancellation algorithm:
        // 1. High-pass filter to reduce low-frequency echo
        // 2. Adaptive noise cancellation based on far-end reference
        // 3. Non-linear processing to suppress residual echo

        let mut output = input.clone();

        // High-pass filter (simple 1-pole filter)
        let alpha = 0.97; // Increased filter coefficient for better high-pass filtering
        let mut prev = 0.0;

        for i in 0..output.len() {
            output[i] = alpha * (output[i] - prev) + prev;
            prev = output[i];
        }

        // Improved echo cancellation using adaptive subtraction
        if !far_end.is_empty() {
            let echo_coef = 0.5; // Increased echo reduction coefficient
            let delay = 100; // Estimated delay between far-end and echo in samples

            // Use the minimum length of both buffers
            let min_len = std::cmp::min(output.len(), far_end.len());

            for i in 0..min_len {
                if i >= delay {
                    // Apply echo cancellation with delay estimation
                    let far_end_idx = if i >= delay { i - delay } else { 0 };
                    if far_end_idx < far_end.len() {
                        // Adaptive echo reduction based on far-end signal energy
                        let far_end_energy = far_end[far_end_idx].abs();
                        let adaptive_coef = echo_coef * (0.5 + far_end_energy);
                        output[i] -= adaptive_coef * far_end[far_end_idx];
                    }
                }
            }

            // Non-linear processing to further reduce residual echo
            for i in 0..output.len() {
                // Apply soft noise gate to suppress low-level residual echo
                if output[i].abs() < 0.05 {
                    output[i] *= 0.5; // Attenuate low-level signals
                }
            }
        }

        // Apply frequency-selective suppression
        // This simple approach reduces mid-range frequencies where echo is often strongest
        if output.len() > 32 {
            // Simple frequency-selective processing by windowing the signal
            let window_size = 32;
            let mut i = 0;

            while i + window_size < output.len() {
                // Calculate energy in this window
                let window_energy = output[i..i + window_size]
                    .iter()
                    .map(|&sample| sample.powi(2))
                    .sum::<f32>()
                    / window_size as f32;

                // Apply stronger suppression to medium-energy windows
                // (likely to be echo rather than direct speech)
                if window_energy > 0.01 && window_energy < 0.1 {
                    for j in i..i + window_size {
                        output[j] *= 0.7; // Selective attenuation
                    }
                }

                i += window_size / 2; // Overlapping windows
            }
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_echo_cancellation() {
        // Create a test signal and its echo version
        let original = generate_test_audio();
        let input_with_echo = generate_test_audio_with_echo();

        // Create processor with echo cancellation enabled
        let mut processor = VoiceProcessor::new().with_echo_cancellation(true);

        // Set the far-end reference (needed for echo cancellation to work)
        processor.set_far_end_audio(&original);

        // Process the audio with echo
        let processed = processor.process(input_with_echo.clone());

        // Calculate echo levels
        let input_echo_level = measure_echo_level(&input_with_echo);
        let processed_echo_level = measure_echo_level(&processed);

        // Debug output to see the values
        println!("Original echo level: {}", input_echo_level);
        println!("Processed echo level: {}", processed_echo_level);

        // Echo level should be reduced - the processed signal should have less echo
        assert!(processed_echo_level < input_echo_level);
    }

    #[test]
    fn test_voice_activity_detection() {
        let processor = VoiceProcessor::new();

        let silence = generate_test_silence();
        assert!(!processor.detect_voice_activity(&silence));

        let speech = generate_test_speech();
        assert!(processor.detect_voice_activity(&speech));
    }

    #[test]
    fn test_mute_functionality() {
        // Test initial state
        let processor = VoiceProcessor::new();
        assert!(!processor.is_muted());

        // Test with_muted builder
        let muted_processor = VoiceProcessor::new().with_muted(true);
        assert!(muted_processor.is_muted());

        // Test set_muted
        let mut processor = VoiceProcessor::new();
        processor.set_muted(true);
        assert!(processor.is_muted());

        // Test toggle_mute
        let mut processor = VoiceProcessor::new();
        assert!(!processor.is_muted());
        let new_state = processor.toggle_mute();
        assert!(new_state);
        assert!(processor.is_muted());
        let new_state = processor.toggle_mute();
        assert!(!new_state);
        assert!(!processor.is_muted());
    }

    #[test]
    fn test_mute_processing() {
        let speech = generate_test_speech();

        // Test unmuted - should return processed audio
        let processor = VoiceProcessor::new();
        let processed = processor.process(speech.clone());
        assert!(!processed.iter().all(|&sample| sample == 0.0));

        // Test muted - should return silence
        let processor = VoiceProcessor::new().with_muted(true);
        let processed = processor.process(speech.clone());
        assert!(processed.iter().all(|&sample| sample == 0.0));
        assert_eq!(processed.len(), speech.len());
    }
}
