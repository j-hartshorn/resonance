// Spectrogram widget
// Displays a audio spectrogram for a participant

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    symbols,
    widgets::{Block, Borders, Widget},
};

pub struct Spectrogram<'a> {
    block: Option<Block<'a>>,
    data: &'a [f32],
    max_value: f32,
    style: Style,
}

impl<'a> Spectrogram<'a> {
    pub fn new(data: &'a [f32]) -> Self {
        Self {
            block: None,
            data,
            max_value: data.iter().fold(0.0f32, |a, &b| a.max(b)),
            style: Style::default().fg(Color::Green),
        }
    }
    
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }
    
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

impl<'a> Widget for Spectrogram<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = self.block.unwrap_or_else(|| Block::default());
        let inner_area = block.inner(area);
        block.render(area, buf);
        
        if inner_area.width < 1 || inner_area.height < 1 || self.data.is_empty() {
            return;
        }
        
        let max_value = if self.max_value == 0.0 { 1.0 } else { self.max_value };
        let bar_width = inner_area.width as usize / self.data.len().max(1);
        
        if bar_width == 0 {
            return;
        }
        
        for (i, &value) in self.data.iter().enumerate() {
            let normalized_value = value / max_value;
            let bar_height = (normalized_value * inner_area.height as f32).round() as u16;
            let bar_height = bar_height.min(inner_area.height);
            
            for j in 0..bar_width {
                let x = inner_area.left() + (i * bar_width + j) as u16;
                
                for y in 0..bar_height {
                    let y = inner_area.bottom() - 1 - y;
                    buf.get_mut(x, y).set_style(self.style);
                    buf.get_mut(x, y).set_symbol(symbols::block::FULL);
                }
            }
        }
    }
}