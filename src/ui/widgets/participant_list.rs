use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, StatefulWidget, Widget},
};
use std::sync::{Arc, Mutex};

/// Represents a participant in the audio session
#[derive(Clone, Debug)]
pub struct Participant {
    pub id: String,
    pub name: String,
    pub is_speaking: bool,
    pub position: (f32, f32, f32), // (x, y, z) position in virtual space
}

impl Participant {
    pub fn new(name: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            is_speaking: false,
            position: (0.0, 0.0, 0.0),
        }
    }

    pub fn with_position(mut self, x: f32, y: f32, z: f32) -> Self {
        self.position = (x, y, z);
        self
    }
}

#[derive(Clone)]
pub struct ParticipantListWidget {
    participants: Arc<Mutex<Vec<Participant>>>,
    state: ListState,
}

impl ParticipantListWidget {
    pub fn new() -> Self {
        Self {
            participants: Arc::new(Mutex::new(Vec::new())),
            state: ListState::default(),
        }
    }

    pub fn set_participants(&self, participants: Vec<Participant>) {
        let mut lock = self.participants.lock().unwrap();
        *lock = participants;
    }

    pub fn get_participants(&self) -> Vec<Participant> {
        let lock = self.participants.lock().unwrap();
        lock.clone()
    }

    pub fn select_next(&mut self) {
        let participants = self.participants.lock().unwrap();
        let i = match self.state.selected() {
            Some(i) => {
                if i >= participants.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn select_previous(&mut self) {
        let participants = self.participants.lock().unwrap();
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    if participants.is_empty() {
                        0
                    } else {
                        participants.len() - 1
                    }
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn selected(&self) -> Option<Participant> {
        let participants = self.participants.lock().unwrap();
        if participants.is_empty() {
            return None;
        }
        self.state.selected().map(|i| participants[i].clone())
    }
}

impl StatefulWidget for ParticipantListWidget {
    type State = ListState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let participants = self.participants.lock().unwrap();

        let items: Vec<ListItem> = participants
            .iter()
            .map(|p| {
                let style = if p.is_speaking {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default()
                };

                let pos_text = format!(
                    "({:.1}, {:.1}, {:.1})",
                    p.position.0, p.position.1, p.position.2
                );

                let line = Line::from(vec![
                    Span::styled(&p.name, style),
                    Span::raw(" "),
                    Span::styled(pos_text, Style::default().fg(Color::DarkGray)),
                ]);

                ListItem::new(line)
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().title("Participants").borders(Borders::ALL))
            .highlight_style(Style::default().fg(Color::Yellow));

        StatefulWidget::render(list, area, buf, state);
    }
}

impl Widget for ParticipantListWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut state = ListState::default();
        StatefulWidget::render(self, area, buf, &mut state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_participant_list_widget() {
        let widget = ParticipantListWidget::new();
        let participants = vec![Participant::new("User1"), Participant::new("User2")];

        widget.set_participants(participants.clone());
        let displayed = widget.get_participants();

        assert_eq!(participants.len(), displayed.len());
        assert_eq!(participants[0].name, displayed[0].name);
    }

    #[test]
    fn test_participant_selection() {
        let mut widget = ParticipantListWidget::new();
        let participants = vec![
            Participant::new("User1"),
            Participant::new("User2"),
            Participant::new("User3"),
        ];

        widget.set_participants(participants.clone());

        // Initially no selection
        assert!(widget.selected().is_none());

        // Select first
        widget.select_next();
        assert_eq!(widget.selected().unwrap().name, "User1");

        // Move to next
        widget.select_next();
        assert_eq!(widget.selected().unwrap().name, "User2");

        // Move to previous
        widget.select_previous();
        assert_eq!(widget.selected().unwrap().name, "User1");
    }
}
