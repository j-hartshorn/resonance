use super::capture::generate_test_mono_audio;

#[derive(Clone)]
pub struct SpatialAudioProcessor {
    source_position: (f32, f32, f32), // (x, y, z) in 3D space
    listener_position: (f32, f32, f32),
    listener_orientation: (f32, f32, f32), // (yaw, pitch, roll) in radians
    room_size: (f32, f32, f32),            // Width, height, depth
    reverb_amount: f32,                    // 0.0-1.0
}

impl SpatialAudioProcessor {
    pub fn new() -> Self {
        Self {
            source_position: (0.0, 0.0, 0.0),
            listener_position: (0.0, 0.0, 0.0),
            listener_orientation: (0.0, 0.0, 0.0),
            room_size: (10.0, 3.0, 10.0), // Default room size in meters
            reverb_amount: 0.3,
        }
    }

    pub fn set_source_position(&mut self, x: f32, y: f32, z: f32) {
        self.source_position = (x, y, z);
    }

    pub fn set_listener_position(&mut self, x: f32, y: f32, z: f32) {
        self.listener_position = (x, y, z);
    }

    pub fn set_listener_orientation(&mut self, yaw: f32, pitch: f32, roll: f32) {
        self.listener_orientation = (yaw, pitch, roll);
    }

    pub fn set_room_size(&mut self, width: f32, height: f32, depth: f32) {
        self.room_size = (width, height, depth);
    }

    pub fn set_reverb_amount(&mut self, amount: f32) {
        self.reverb_amount = amount.clamp(0.0, 1.0);
    }

    pub fn process(&self, mono_input: &[f32]) -> Vec<f32> {
        // In a real implementation, this would use audionimbus or another
        // spatial audio library for proper 3D audio processing

        // For testing, we'll do a simple stereo panning based on x-position
        let mut stereo_output = Vec::with_capacity(mono_input.len() * 2);

        // Calculate relative position (only using x-coordinate for simplicity)
        let rel_x = self.source_position.0 - self.listener_position.0;

        // Convert to pan value between -1.0 (full left) and 1.0 (full right)
        let pan = rel_x.clamp(-1.0, 1.0);

        // Calculate left/right gains using equal-power panning
        let angle = (pan + 1.0) * std::f32::consts::PI / 4.0; // 0 to π/2
        let left_gain = angle.cos();
        let right_gain = angle.sin();

        // Apply panning to create stereo output
        for &sample in mono_input {
            stereo_output.push(sample * left_gain); // Left channel
            stereo_output.push(sample * right_gain); // Right channel
        }

        // Add basic reverb if enabled
        if self.reverb_amount > 0.0 {
            self.apply_reverb(&mut stereo_output);
        }

        stereo_output
    }

    fn apply_reverb(&self, stereo_output: &mut Vec<f32>) {
        // Very simple reverb simulation for testing
        // In a real implementation, this would use more sophisticated algorithms

        let delay_samples = (self.room_size.0 * 10.0) as usize; // Simple approximation
        if delay_samples >= stereo_output.len() / 4 {
            return; // Avoid excessive delay for testing
        }

        let reverb_gain = self.reverb_amount * 0.3; // Scale down to avoid clipping

        let original = stereo_output.clone();
        for i in delay_samples..stereo_output.len() {
            stereo_output[i] += original[i - delay_samples] * reverb_gain;
        }
    }
}

// Helper function to measure stereo channel levels
pub fn measure_stereo_levels(stereo_audio: &[f32]) -> (f32, f32) {
    if stereo_audio.len() < 2 {
        return (0.0, 0.0);
    }

    let mut left_sum = 0.0;
    let mut right_sum = 0.0;

    for i in (0..stereo_audio.len()).step_by(2) {
        if i + 1 < stereo_audio.len() {
            left_sum += stereo_audio[i].abs();
            right_sum += stereo_audio[i + 1].abs();
        }
    }

    let count = stereo_audio.len() as f32 / 2.0;
    (left_sum / count, right_sum / count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spatial_positioning() {
        let mut processor = SpatialAudioProcessor::new();
        let mono_input = generate_test_mono_audio();

        // Position sound to the right
        processor.set_source_position(1.0, 0.0, 0.0);
        let right_biased = processor.process(&mono_input);

        // Position sound to the left
        processor.set_source_position(-1.0, 0.0, 0.0);
        let left_biased = processor.process(&mono_input);

        // Check that positioning works (right channel louder when positioned right)
        let (left_level_when_right, right_level_when_right) = measure_stereo_levels(&right_biased);
        let (left_level_when_left, right_level_when_left) = measure_stereo_levels(&left_biased);

        assert!(right_level_when_right > left_level_when_right);
        assert!(left_level_when_left > right_level_when_left);
    }
}
