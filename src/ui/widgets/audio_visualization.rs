use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    symbols,
    widgets::{Block, Borders, Widget},
};
use rustfft::{num_complex::Complex, FftPlanner};
use std::sync::{Arc, Mutex};

/// A widget to visualize audio data as a frequency spectrum (spectrogram)
#[derive(Clone)]
pub struct AudioVisualizationWidget {
    audio_data: Arc<Mutex<Vec<f32>>>,
    peak_levels: Arc<Mutex<Vec<f32>>>,
    spectrum_data: Arc<Mutex<Vec<f32>>>,
    spectrum_history: Arc<Mutex<Vec<Vec<f32>>>>,
    max_samples: usize,
    num_bins: usize,
    history_length: usize,
}

impl AudioVisualizationWidget {
    pub fn new() -> Self {
        Self {
            audio_data: Arc::new(Mutex::new(Vec::new())),
            peak_levels: Arc::new(Mutex::new(Vec::new())),
            spectrum_data: Arc::new(Mutex::new(Vec::new())),
            spectrum_history: Arc::new(Mutex::new(Vec::new())),
            max_samples: 2048, // Increased for better low-frequency resolution
            num_bins: 24,      // Number of frequency bins to display
            history_length: 3, // Number of frames to average
        }
    }

    /// Update the audio data to be visualized
    pub fn update_data(&self, data: &[f32]) {
        let mut audio_data = self.audio_data.lock().unwrap();

        // We need enough samples for FFT, ideally a power of 2
        if data.len() > self.max_samples {
            let step = data.len() / self.max_samples;
            *audio_data = data
                .iter()
                .step_by(step)
                .take(self.max_samples)
                .cloned()
                .collect();
        } else {
            *audio_data = data.to_vec();
        }

        // Update peak levels (moving average)
        let mut peaks = self.peak_levels.lock().unwrap();
        if peaks.len() >= 8 {
            peaks.remove(0);
        }

        let max_amplitude = data
            .iter()
            .map(|sample| sample.abs())
            .fold(0.0f32, |a, b| a.max(b));

        peaks.push(max_amplitude);

        // Compute frequency spectrum using FFT
        self.compute_spectrum(&audio_data);
    }

    /// Compute the frequency spectrum using FFT
    fn compute_spectrum(&self, audio_data: &[f32]) {
        if audio_data.is_empty() {
            return;
        }

        // Prepare FFT data - need to convert to complex numbers
        let mut fft_input: Vec<Complex<f32>> = audio_data
            .iter()
            .map(|&sample| Complex::new(sample, 0.0))
            .collect();

        // Pad to power of 2 if needed
        let fft_size = fft_input.len().next_power_of_two();
        fft_input.resize(fft_size, Complex::new(0.0, 0.0));

        // Apply window function (Hann window) to reduce spectral leakage
        for i in 0..fft_input.len() {
            let window = 0.5
                * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / fft_input.len() as f32).cos());
            fft_input[i] = fft_input[i] * window;
        }

        // Create FFT planner and run FFT
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(fft_size);

        // Perform FFT in-place
        fft.process(&mut fft_input);

        // Calculate magnitude of each complex output (sqrt(real² + imag²))
        // We only need the first half, as FFT output is symmetric for real input
        let mut magnitudes: Vec<f32> = fft_input
            .iter()
            .take(fft_size / 2)
            .map(|c| (c.norm_sqr()).sqrt())
            .collect();

        // Scale magnitudes logarithmically (dB scale) with improved dynamics
        for mag in &mut magnitudes {
            // Convert to dB scale (20 * log10(mag))
            // Adding a small value to avoid log(0)
            *mag = 20.0 * ((*mag + 1e-10).log10());

            // Apply a more dramatic curve to suppress small amplitudes
            // and enhance medium-to-loud sounds (better for speech)
            if *mag < -60.0 {
                *mag = 0.0; // Noise gate - cut off very quiet sounds
            } else {
                // Normalize to 0.0 - 1.0 range with emphasis on speech levels
                *mag = (*mag + 60.0) / 60.0; // Map -60dB..0dB to 0..1

                // Apply non-linear curve to enhance mid-range values (speech volumes)
                *mag = (*mag * *mag) * 1.3; // Square to enhance contrast
                *mag = mag.max(0.0).min(1.0); // Clamp to 0-1
            }
        }

        // Bin the magnitudes into frequency bands for visualization
        // Use a logarithmic frequency scale to focus more on lower frequencies (where voice is)
        let mut new_spectrum = Vec::with_capacity(self.num_bins);

        // Frequency ranges oriented towards vocal content
        // Speech is typically 85Hz-255Hz for fundamentals, with content up to ~8kHz
        if !magnitudes.is_empty() {
            let sample_rate = 44100.0; // Assuming 44.1kHz sample rate
            let nyquist = sample_rate / 2.0;
            let freq_per_bin = nyquist / magnitudes.len() as f32;

            // Define vocal-focused frequency bands (in Hz)
            let freq_bands = [
                // Sub-bass and bass (male voice fundamentals)
                80.0, 100.0, 125.0, 160.0, 200.0,
                // Mid-range (female voice fundamentals)
                250.0, 315.0, 400.0, 500.0, 630.0,
                // Upper mid-range (important for speech clarity)
                800.0, 1000.0, 1250.0, 1600.0, 2000.0, // Presence and brilliance
                2500.0, 3150.0, 4000.0, 5000.0, 6300.0, // Upper frequencies
                8000.0, 10000.0, 12500.0, 16000.0,
            ];

            // Calculate spectrum based on these bands
            for (i, &freq) in freq_bands.iter().enumerate() {
                if i == freq_bands.len() - 1 || i >= self.num_bins {
                    break;
                }

                let start_bin = (freq / freq_per_bin) as usize;
                let end_bin = (freq_bands[i + 1] / freq_per_bin) as usize;

                let start = start_bin.min(magnitudes.len() - 1);
                let end = end_bin.min(magnitudes.len());

                if start < end {
                    let avg_magnitude =
                        magnitudes[start..end].iter().sum::<f32>() / (end - start) as f32;
                    new_spectrum.push(avg_magnitude);
                } else {
                    new_spectrum.push(0.0);
                }
            }

            // Ensure we have exactly num_bins
            new_spectrum.resize(self.num_bins, 0.0);
        } else {
            // Fill with zeros if no data
            new_spectrum.resize(self.num_bins, 0.0);
        }

        // Apply temporal smoothing using a moving average
        let mut history = self.spectrum_history.lock().unwrap();

        // Add new spectrum to history
        history.push(new_spectrum);

        // Keep only the latest history_length frames
        while history.len() > self.history_length {
            history.remove(0);
        }

        // Calculate the moving average
        let mut spectrum = self.spectrum_data.lock().unwrap();
        spectrum.clear();

        if !history.is_empty() {
            spectrum.resize(self.num_bins, 0.0);

            // Calculate weighted moving average with more weight to recent frames
            let mut total_weight = 0.0;

            for (i, frame) in history.iter().enumerate() {
                let weight = (i + 1) as f32; // More recent frames get higher weight
                total_weight += weight;

                for (bin, &value) in frame.iter().enumerate() {
                    if bin < spectrum.len() {
                        spectrum[bin] += value * weight;
                    }
                }
            }

            // Normalize by total weight
            if total_weight > 0.0 {
                for bin in spectrum.iter_mut() {
                    *bin /= total_weight;
                }
            }
        }
    }

    /// Get the current peak levels
    pub fn get_peak_levels(&self) -> Vec<f32> {
        let peaks = self.peak_levels.lock().unwrap();
        peaks.clone()
    }

    /// Set the maximum number of samples to use for FFT
    pub fn with_max_samples(mut self, max: usize) -> Self {
        self.max_samples = max;
        self
    }

    /// Set the number of frequency bins to display
    pub fn with_num_bins(mut self, bins: usize) -> Self {
        self.num_bins = bins;
        self
    }

    /// Set the length of the moving average history for smoothing
    pub fn with_history_length(mut self, length: usize) -> Self {
        self.history_length = length;
        self
    }
}

impl Widget for AudioVisualizationWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Draw a box around the widget
        Block::default()
            .title("Vocal Frequency Spectrum")
            .borders(Borders::ALL)
            .render(area, buf);

        let spectrum_data = self.spectrum_data.lock().unwrap();
        if spectrum_data.is_empty() {
            return;
        }

        // Inner drawing area
        let inner_area = Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );

        if inner_area.width == 0 || inner_area.height == 0 {
            return;
        }

        // Draw frequency spectrum bars
        let max_height = inner_area.height;
        let bar_width = inner_area.width / spectrum_data.len() as u16;
        let bar_width = bar_width.max(1); // Ensure minimum width of 1

        for (i, &magnitude) in spectrum_data.iter().enumerate() {
            let bar_height = (magnitude * magnitude * max_height as f32) as u16; // Apply quadratic scaling
            let bar_height = bar_height.min(max_height);

            // Skip if no height
            if bar_height == 0 {
                continue;
            }

            // Calculate the x position for this bar
            let x = inner_area.x + (i as u16 * bar_width);

            // Draw the bar from bottom to top
            for y in 0..bar_height {
                let current_y = inner_area.y + inner_area.height - y - 1;

                // Color gradient based on frequency and amplitude
                let style = if i < spectrum_data.len() / 4 {
                    // Low frequencies (male voice range) - deep blue to blue
                    Style::default().fg(Color::Blue)
                } else if i < spectrum_data.len() / 2 {
                    // Low-mid frequencies (female voice range) - blue to green
                    Style::default().fg(Color::Green)
                } else if i < 3 * spectrum_data.len() / 4 {
                    // Mid-high frequencies (consonants, clarity) - green to yellow
                    Style::default().fg(Color::Yellow)
                } else {
                    // High frequencies - yellow to red
                    Style::default().fg(Color::Red)
                };

                // Draw the portion of the bar at this height
                for bar_x in 0..bar_width {
                    if x + bar_x < inner_area.x + inner_area.width {
                        buf.get_mut(x + bar_x, current_y)
                            .set_symbol(symbols::block::FULL)
                            .set_style(style);
                    }
                }
            }
        }

        // Draw peak meter on the right
        let peaks = self.peak_levels.lock().unwrap();
        if !peaks.is_empty() {
            let peak_x = inner_area.x + inner_area.width - 2;
            let peak_height = inner_area.height;

            // Get average peak
            let avg_peak: f32 = peaks.iter().sum::<f32>() / peaks.len() as f32;
            let peak_level = (avg_peak * peak_height as f32) as u16;

            for y in 0..peak_height {
                let current_y = inner_area.y + peak_height - y - 1;

                let style = if y < peak_level {
                    if y > peak_height * 3 / 4 {
                        Style::default().fg(Color::Red)
                    } else if y > peak_height / 2 {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default().fg(Color::Green)
                    }
                } else {
                    Style::default().fg(Color::DarkGray)
                };

                buf.get_mut(peak_x, current_y)
                    .set_symbol(symbols::block::FULL)
                    .set_style(style);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generate_test_audio_data() -> Vec<f32> {
        // Generate a simple sine wave as test data
        (0..1000).map(|i| (i as f32 / 50.0).sin() * 0.5).collect()
    }

    #[test]
    fn test_audio_visualization_widget() {
        let widget = AudioVisualizationWidget::new();
        let audio_data = generate_test_audio_data();

        widget.update_data(&audio_data);
        let peaks = widget.get_peak_levels();

        assert!(!peaks.is_empty());
        // Maximum amplitude of our test sine wave should be around 0.5
        assert!(peaks[0] > 0.4 && peaks[0] < 0.6);
    }

    #[test]
    fn test_audio_downsampling() {
        let widget = AudioVisualizationWidget::new().with_max_samples(10);
        let audio_data = generate_test_audio_data();

        widget.update_data(&audio_data);

        // Check that data was downsampled
        let stored_data = widget.audio_data.lock().unwrap();
        assert!(stored_data.len() <= 10);
    }
}
