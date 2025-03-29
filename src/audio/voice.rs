use super::capture::{
    generate_test_audio_with_echo, generate_test_silence, generate_test_speech, measure_echo_level,
};

#[derive(Clone)]
pub struct VoiceProcessor {
    vad_threshold: f32,
    echo_cancellation_enabled: bool,
    muted: bool,
}

impl VoiceProcessor {
    pub fn new() -> Self {
        Self {
            vad_threshold: 0.1, // Arbitrary threshold for voice activity detection
            echo_cancellation_enabled: true,
            muted: false,
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

        // In a real implementation, this would use webrtc-audio-processing
        // or another library for actual voice processing

        // For testing purposes, we'll implement basic simulations

        let mut output = input.clone();

        // Apply echo cancellation if enabled
        if self.echo_cancellation_enabled {
            output = self.apply_echo_cancellation(output);
        }

        output
    }

    pub fn detect_voice_activity(&self, audio: &[f32]) -> bool {
        // Simple VAD: check if the audio energy exceeds a threshold
        let energy = audio.iter().map(|&sample| sample.powi(2)).sum::<f32>() / audio.len() as f32;
        energy > self.vad_threshold
    }

    fn apply_echo_cancellation(&self, input: Vec<f32>) -> Vec<f32> {
        // Simple simulation of echo cancellation
        // In a real implementation, this would use more sophisticated algorithms

        let mut output = input.clone();

        // Basic highpass filter to reduce low-frequency echo components
        let alpha = 0.8; // Filter coefficient
        let mut prev = 0.0;

        for i in 0..output.len() {
            output[i] = alpha * (output[i] - prev) + prev;
            prev = output[i];
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
