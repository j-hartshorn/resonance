use super::capture::generate_test_mono_audio;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct SpatialAudioProcessor {
    source_position: (f32, f32, f32), // (x, y, z) in 3D space
    listener_position: (f32, f32, f32),
    listener_orientation: (f32, f32, f32), // (yaw, pitch, roll) in radians
    room_size: (f32, f32, f32),            // Width, height, depth
    reverb_amount: f32,                    // 0.0-1.0

    // Audio processing parameters
    sample_rate: u32,
}

impl SpatialAudioProcessor {
    pub fn new() -> Self {
        Self {
            source_position: (0.0, 0.0, 0.0),
            listener_position: (0.0, 0.0, 0.0),
            listener_orientation: (0.0, 0.0, 0.0),
            room_size: (10.0, 3.0, 10.0), // Default room size in meters
            reverb_amount: 0.3,
            sample_rate: 48000, // Default 48kHz
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

    /// Process mono audio into spatial stereo audio
    ///
    /// This implementation uses a simplified HRTF-like approach with:
    /// 1. Interaural Time Difference (ITD) - sound reaches the further ear later
    /// 2. Interaural Level Difference (ILD) - sound is quieter at the further ear
    /// 3. Basic frequency-dependent filtering
    /// 4. Simple distance attenuation
    /// 5. Room reverb simulation
    pub fn process(&self, mono_input: &[f32]) -> Vec<f32> {
        // Calculate relative position between source and listener
        let rel_x = self.source_position.0 - self.listener_position.0;
        let rel_y = self.source_position.1 - self.listener_position.1;
        let rel_z = self.source_position.2 - self.listener_position.2;

        // Convert to the listener's coordinate system
        // Apply yaw rotation around Y axis
        let yaw = self.listener_orientation.0;
        let rotated_x = rel_x * yaw.cos() + rel_z * yaw.sin();
        let rotated_z = -rel_x * yaw.sin() + rel_z * yaw.cos();

        // Calculate distance for attenuation
        let distance = (rel_x * rel_x + rel_y * rel_y + rel_z * rel_z).sqrt();

        // Distance attenuation (inverse square law, clamped for near sources)
        let min_distance = 0.3; // Minimum distance before attenuation begins
        let distance_attenuation = if distance < min_distance {
            1.0
        } else {
            min_distance / distance
        };

        // Calculate azimuth angle in the horizontal plane
        let azimuth = rotated_z.atan2(rotated_x);

        // Convert azimuth to range [-1.0, 1.0] where -1 is full left, 1 is full right
        // azimuth = 0 is in front, π is behind, π/2 is to the right, -π/2 is to the left
        let pan = (azimuth / std::f32::consts::PI).clamp(-1.0, 1.0);

        // Apply binaural processing
        let stereo_output = self.apply_binaural_processing(mono_input, pan, distance);

        // Apply reverb if enabled
        if self.reverb_amount > 0.0 {
            self.apply_reverb(&stereo_output)
        } else {
            stereo_output
        }
    }

    // Apply more realistic binaural processing
    fn apply_binaural_processing(&self, mono_input: &[f32], pan: f32, distance: f32) -> Vec<f32> {
        let mut stereo_output = Vec::with_capacity(mono_input.len() * 2);

        // Calculate left/right gains using equal-power panning
        let angle = (pan + 1.0) * std::f32::consts::PI / 4.0; // 0 to π/2
        let left_gain = angle.cos();
        let right_gain = angle.sin();

        // Calculate ITD (Interaural Time Difference) - delay to the further ear
        // Head width approximation: 0.15 meters (15 cm)
        let head_width = 0.15;
        let max_delay_time = head_width / 343.0; // 343 m/s is speed of sound
        let delay_samples = (max_delay_time * self.sample_rate as f32 * pan.abs()).ceil() as usize;

        // For negative pan (sound from left), delay right ear
        // For positive pan (sound from right), delay left ear
        let (left_delay, right_delay) = if pan < 0.0 {
            (0, delay_samples)
        } else {
            (delay_samples, 0)
        };

        // Create delay buffers filled with zeros
        let left_delay_buffer = vec![0.0; left_delay];
        let right_delay_buffer = vec![0.0; right_delay];

        // Apply frequency-dependent filtering (simplified HRTF)
        // High frequencies attenuate more when going around the head
        let mut left_filtered = mono_input.to_vec();
        let mut right_filtered = mono_input.to_vec();

        // Simple lowpass filter for the shadowed ear
        if pan < 0.0 {
            // Sound from left, apply filter to right ear
            self.apply_lowpass_filter(&mut right_filtered, pan.abs());
        } else {
            // Sound from right, apply filter to left ear
            self.apply_lowpass_filter(&mut left_filtered, pan.abs());
        }

        // Apply gains after filtering
        for i in 0..left_filtered.len() {
            left_filtered[i] *= left_gain * distance;
            right_filtered[i] *= right_gain * distance;
        }

        // Combine the processed audio with the delay
        // Process left channel
        if left_delay > 0 {
            // If we need to delay the left channel, start with zeros
            stereo_output.extend(left_delay_buffer.iter().map(|&s| s));
            stereo_output.extend(left_filtered.iter().map(|&s| s));
        } else {
            stereo_output.extend(left_filtered.iter().map(|&s| s));
            // Pad with zeros at the end to maintain proper length
            stereo_output.extend(vec![0.0; right_delay]);
        }

        // Process right channel
        if right_delay > 0 {
            // If we need to delay the right channel, start with zeros
            stereo_output.extend(right_delay_buffer.iter().map(|&s| s));
            stereo_output.extend(right_filtered.iter().map(|&s| s));
        } else {
            stereo_output.extend(right_filtered.iter().map(|&s| s));
            // Pad with zeros at the end to maintain proper length
            stereo_output.extend(vec![0.0; left_delay]);
        }

        stereo_output
    }

    // Simple lowpass filter
    fn apply_lowpass_filter(&self, samples: &mut [f32], strength: f32) {
        // Higher strength means more filtering (attenuating high frequencies)
        let alpha = 0.5 + 0.45 * strength;
        let mut prev = 0.0;

        for i in 0..samples.len() {
            let curr = samples[i];
            samples[i] = alpha * prev + (1.0 - alpha) * curr;
            prev = samples[i];
        }
    }

    // Create a realistic-sounding reverb based on room size
    fn apply_reverb(&self, input: &[f32]) -> Vec<f32> {
        let mut output = input.to_vec();

        // Calculate room size-based delay and decay
        let max_dimension = self.room_size.0.max(self.room_size.2);
        let room_volume = self.room_size.0 * self.room_size.1 * self.room_size.2;

        // Calculate reflection times based on room dimensions
        let num_reflections = 3; // Number of reflections to simulate
        let mut delays = Vec::with_capacity(num_reflections);
        let mut gains = Vec::with_capacity(num_reflections);

        for i in 1..=num_reflections {
            // Calculate delay for this reflection
            // More distant reflections have longer delays
            let delay_meters = max_dimension * 2.0 * i as f32;
            let delay_time = delay_meters / 343.0; // 343 m/s is speed of sound
            let delay_samples = (delay_time * self.sample_rate as f32) as usize;

            // Smaller rooms have stronger initial reflections
            let base_attenuation = 1.0 / (i as f32 * 2.0);
            let size_factor = 30.0 / room_volume.min(30.0); // Normalized for reasonable room sizes
            let gain = base_attenuation * size_factor * self.reverb_amount;

            delays.push(delay_samples);
            gains.push(gain);
        }

        // Add reflections
        let original = input.to_vec();

        for i in 0..num_reflections {
            let delay = delays[i];
            let gain = gains[i];

            // Skip if delay is too large
            if delay >= original.len() / 2 {
                continue;
            }

            // Add delayed reverb signal
            for j in delay..original.len() {
                output[j] += original[j - delay] * gain;
            }
        }

        output
    }

    // Set the sample rate for the processor
    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate;
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
        assert!((vec_0_to_1.0 + vec_1_to_0.0).abs() < 0.001);
        assert!((vec_0_to_1.2 + vec_1_to_0.2).abs() < 0.001);
    }
}
