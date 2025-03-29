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

    /// Arrange participants in a virtual circle
    ///
    /// Returns a vector of (x, y, z) positions for each participant,
    /// and sets the current user's listener position and orientation.
    ///
    /// # Arguments
    /// * `participant_count` - Total number of participants
    /// * `current_user_index` - Index of the current user in the circle (0-based)
    pub fn arrange_participants_in_circle(
        &mut self,
        participant_count: usize,
        current_user_index: usize,
    ) -> Vec<(f32, f32, f32)> {
        // Use a reasonable default radius for the circle (in meters)
        let radius = 2.0;

        // Create positions for all participants
        let mut positions = Vec::with_capacity(participant_count);

        for i in 0..participant_count {
            // Calculate angle in radians for this participant
            let angle = 2.0 * std::f32::consts::PI * (i as f32) / (participant_count as f32);

            // Calculate position on the circle (y is up, so we use x and z for the circle)
            let x = radius * angle.cos();
            let z = radius * angle.sin();

            positions.push((x, 0.0, z));
        }

        // Update listener position and orientation for the current user
        if participant_count > 0 && current_user_index < participant_count {
            // Set listener position to the current user's position in the circle
            self.listener_position = positions[current_user_index];

            // Calculate the angle to face the center of the circle
            // This is the opposite of the position angle
            let angle = 2.0 * std::f32::consts::PI * (current_user_index as f32)
                / (participant_count as f32);

            // Set orientation to face the center (negative to face inward)
            // Only setting yaw (rotation around y-axis), keeping pitch and roll at 0
            self.listener_orientation = (-angle, 0.0, 0.0);
        }

        positions
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

    #[test]
    fn test_circular_positioning() {
        let mut processor = SpatialAudioProcessor::new();

        // Test with 4 participants
        let positions = processor.arrange_participants_in_circle(4, 0);

        // Verify we have 4 positions
        assert_eq!(positions.len(), 4);

        // Verify positions form a circle
        // For each position, check distance from center is roughly the same
        let radius = 2.0; // Expected radius
        for (x, _, z) in &positions {
            let distance = (x * x + z * z).sqrt();
            assert!((distance - radius).abs() < 0.001); // Allow small floating-point error
        }

        // Check listener position is set to the current user's position
        assert_eq!(processor.listener_position, positions[0]);
    }

    #[test]
    fn test_consistent_relative_positioning() {
        let mut processor = SpatialAudioProcessor::new();

        // Arrange 3 participants in a circle
        let positions = processor.arrange_participants_in_circle(3, 0);

        // For 3 participants in a circle at radius 2.0:
        // User 0: (2.0, 0.0, 0.0) - at 0 degrees
        // User 1: (-1.0, 0.0, 1.732) - at 120 degrees
        // User 2: (-1.0, 0.0, -1.732) - at 240 degrees

        // Get positions after arrangement from user 0's perspective
        let pos_0 = processor.listener_position;

        // Calculate vectors from user 0 to other users
        let vec_0_to_1 = (
            positions[1].0 - pos_0.0,
            positions[1].1 - pos_0.1,
            positions[1].2 - pos_0.2,
        );

        let vec_0_to_2 = (
            positions[2].0 - pos_0.0,
            positions[2].1 - pos_0.1,
            positions[2].2 - pos_0.2,
        );

        // Now arrange from user 1's perspective
        let positions_from_1 = processor.arrange_participants_in_circle(3, 1);
        let pos_1 = processor.listener_position;

        // Calculate vector from user 1 to user 0
        let vec_1_to_0 = (
            positions_from_1[0].0 - pos_1.0,
            positions_from_1[0].1 - pos_1.1,
            positions_from_1[0].2 - pos_1.2,
        );

        // Verify consistency in spatial relationship:
        // If user 1 is to the left of user 0, then user 0 should be to the right of user 1
        // In a circle, the vectors should be approximately opposite

        // Sum of the vectors should be close to zero for opposite directions
        let sum_x = vec_0_to_1.0 + vec_1_to_0.0;
        let sum_z = vec_0_to_1.2 + vec_1_to_0.2;

        // Allow some floating point error
        assert!(sum_x.abs() < 0.001, "X vector inconsistency: {}", sum_x);
        assert!(sum_z.abs() < 0.001, "Z vector inconsistency: {}", sum_z);

        // Verify that the dot product of the vectors is negative (pointing in opposite directions)
        let dot_product = vec_0_to_1.0 * vec_1_to_0.0 + vec_0_to_1.2 * vec_1_to_0.2;
        assert!(
            dot_product < 0.0,
            "Vectors should point in opposite directions, dot product: {}",
            dot_product
        );
    }
}
