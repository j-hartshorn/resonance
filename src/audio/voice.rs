use super::capture::{
    generate_test_audio_with_echo, generate_test_silence, generate_test_speech, measure_echo_level,
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
        // Basic echo cancellation algorithm:
        // 1. High-pass filter to reduce low-frequency echo
        // 2. Adaptive noise cancellation based on far-end reference

        let mut output = input.clone();

        // High-pass filter (simple 1-pole filter)
        let alpha = 0.95; // Filter coefficient
        let mut prev = 0.0;

        for i in 0..output.len() {
            output[i] = alpha * (output[i] - prev) + prev;
            prev = output[i];
        }

        // Simple echo cancellation using scaled subtraction
        // This is a simplified version - real implementations are more complex
        if !far_end.is_empty() {
            let echo_coef = 0.3; // Echo reduction coefficient

            // Use the minimum length of both buffers
            let min_len = std::cmp::min(output.len(), far_end.len());

            for i in 0..min_len {
                // Simple echo reduction by subtracting scaled far-end signal
                output[i] -= echo_coef * far_end[i];
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
        let processor = VoiceProcessor::new();
        let input = generate_test_audio_with_echo();
        let processed = processor.process(input.clone());

        // Echo level should be reduced
        assert!(measure_echo_level(&processed) < measure_echo_level(&input));
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
