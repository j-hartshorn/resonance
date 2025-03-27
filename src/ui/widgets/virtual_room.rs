// Virtual room widget
// Displays participants in a 2D representation of the virtual space

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Widget},
};

use crate::app::session::Participant;

pub struct VirtualRoom<'a> {
    block: Option<Block<'a>>,
    participants: &'a [Participant],
    local_id: &'a str,
    bounds: ((f32, f32), (f32, f32)), // ((min_x, min_y), (max_x, max_y))
}

impl<'a> VirtualRoom<'a> {
    pub fn new(participants: &'a [Participant], local_id: &'a str) -> Self {
        Self {
            block: None,
            participants,
            local_id,
            bounds: ((-5.0, -5.0), (5.0, 5.0)),
        }
    }
    
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }
    
    pub fn bounds(mut self, min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> Self {
        self.bounds = ((min_x, min_y), (max_x, max_y));
        self
    }
}

impl<'a> Widget for VirtualRoom<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = self.block.unwrap_or_else(|| Block::default());
        let inner_area = block.inner(area);
        block.render(area, buf);
        
        if inner_area.width < 3 || inner_area.height < 3 || self.participants.is_empty() {
            return;
        }
        
        // Map from world coordinates to screen coordinates
        let ((min_x, min_y), (max_x, max_y)) = self.bounds;
        let x_scale = inner_area.width as f32 / (max_x - min_x);
        let y_scale = inner_area.height as f32 / (max_y - min_y);
        
        // Draw a border for the virtual room
        let border_style = Style::default().fg(Color::Gray);
        for x in inner_area.left()..inner_area.right() {
            buf.get_mut(x, inner_area.top()).set_style(border_style);
            buf.get_mut(x, inner_area.bottom() - 1).set_style(border_style);
        }
        for y in inner_area.top()..inner_area.bottom() {
            buf.get_mut(inner_area.left(), y).set_style(border_style);
            buf.get_mut(inner_area.right() - 1, y).set_style(border_style);
        }
        
        // Draw participants
        for participant in self.participants {
            let x_pos = ((participant.position.0 - min_x) * x_scale) as u16;
            let y_pos = ((participant.position.1 - min_y) * y_scale) as u16;
            
            // Ensure the position is within bounds
            let x = inner_area.left() + x_pos.min(inner_area.width - 1);
            let y = inner_area.top() + y_pos.min(inner_area.height - 1);
            
            // Choose style and symbol based on participant state
            let style = if participant.id == self.local_id {
                Style::default().fg(Color::Cyan)
            } else if participant.is_speaking {
                Style::default().fg(Color::Green)
            } else if participant.muted {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::White)
            };
            
            let symbol = if participant.id == self.local_id { "⭐" } else { "●" };
            
            // Draw the participant
            if x < inner_area.right() && y < inner_area.bottom() {
                buf.get_mut(x, y).set_style(style);
                buf.get_mut(x, y).set_symbol(symbol);
                
                // Draw participant name if there's space
                if x + 2 < inner_area.right() && participant.name.len() + x as usize + 2 <= inner_area.right() as usize {
                    for (i, c) in format!(" {}", participant.name).chars().enumerate() {
                        if x + i as u16 + 1 < inner_area.right() {
                            buf.get_mut(x + i as u16 + 1, y).set_style(style);
                            buf.get_mut(x + i as u16 + 1, y).set_symbol(&c.to_string());
                        }
                    }
                }
            }
        }
    }
}