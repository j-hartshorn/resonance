// Participant list widget
// Displays the list of participants in the session

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Span,
    widgets::{Block, Borders, List, ListItem, ListState, StatefulWidget, Widget},
};

use crate::app::session::Participant;

pub struct ParticipantList<'a> {
    block: Option<Block<'a>>,
    participants: &'a [Participant],
    highlight_style: Style,
}

impl<'a> ParticipantList<'a> {
    pub fn new(participants: &'a [Participant]) -> Self {
        Self {
            block: None,
            participants,
            highlight_style: Style::default().fg(Color::Yellow),
        }
    }
    
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }
    
    pub fn highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = style;
        self
    }
}

impl<'a> Widget for ParticipantList<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = self.block.unwrap_or_else(|| Block::default());
        let inner_area = block.inner(area);
        block.render(area, buf);
        
        if inner_area.height < 1 {
            return;
        }
        
        let items: Vec<ListItem> = self.participants
            .iter()
            .map(|p| {
                let status = if p.is_speaking {
                    "🗣️ "
                } else if p.muted {
                    "🔇 "
                } else {
                    "   "
                };
                
                let text = format!("{}{} ({:.1}, {:.1}, {:.1})", 
                    status, 
                    p.name, 
                    p.position.0, 
                    p.position.1, 
                    p.position.2
                );
                
                ListItem::new(Span::raw(text))
            })
            .collect();
        
        List::new(items)
            .render(inner_area, buf);
    }
}