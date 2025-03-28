use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    symbols,
    widgets::{Block, Borders, Widget},
};
use std::sync::{Arc, Mutex};

/// A simple widget to visualize audio data, showing a waveform-like display
#[derive(Clone)]
pub struct AudioVisualizationWidget {
    audio_data: Arc<Mutex<Vec<f32>>>,
    peak_levels: Arc<Mutex<Vec<f32>>>,
    max_samples: usize,
}

impl AudioVisualizationWidget {
    pub fn new() -> Self {
        Self {
            audio_data: Arc::new(Mutex::new(Vec::new())),
            peak_levels: Arc::new(Mutex::new(Vec::new())),
            max_samples: 100,
        }
    }

    /// Update the audio data to be visualized
    pub fn update_data(&self, data: &[f32]) {
        let mut audio_data = self.audio_data.lock().unwrap();

        // Downsample if needed to max_samples points
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
    }

    /// Get the current peak levels
    pub fn get_peak_levels(&self) -> Vec<f32> {
        let peaks = self.peak_levels.lock().unwrap();
        peaks.clone()
    }

    /// Set the maximum number of samples to visualize
    pub fn with_max_samples(mut self, max: usize) -> Self {
        self.max_samples = max;
        self
    }
}

impl Widget for AudioVisualizationWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Draw a box around the widget
        Block::default()
            .title("Audio Levels")
            .borders(Borders::ALL)
            .render(area, buf);

        let audio_data = self.audio_data.lock().unwrap();
        if audio_data.is_empty() {
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

        // Draw waveform in the center of the area
        let center_y = inner_area.y + inner_area.height / 2;
        let max_height = inner_area.height / 2;

        let points_to_draw = std::cmp::min(audio_data.len(), inner_area.width as usize);
        let step = audio_data.len() as f32 / points_to_draw as f32;

        for i in 0..points_to_draw {
            let x = inner_area.x + i as u16;

            // Get sample at position
            let sample_idx = (i as f32 * step) as usize;
            let sample = audio_data[sample_idx];

            // Scale sample to available height
            let sample_height = (sample.abs() * max_height as f32) as u16;

            // Draw a character representing amplitude
            let y = if sample >= 0.0 {
                center_y.saturating_sub(sample_height)
            } else {
                center_y + 1
            };

            let height = if sample_height == 0 { 1 } else { sample_height };

            for h in 0..height {
                let current_y = if sample >= 0.0 {
                    y + h
                } else {
                    center_y + 1 + h
                };

                if current_y < inner_area.y + inner_area.height {
                    let style = Style::default().fg(if sample >= 0.0 {
                        Color::Green
                    } else {
                        Color::Red
                    });

                    buf.get_mut(x, current_y)
                        .set_symbol(symbols::block::FULL)
                        .set_style(style);
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
