use crate::widgets::{EmojiGrid, EmojiGridState, InputBox, InputBoxState};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, StatefulWidget, Widget},
};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EmojiPickerState {
    pub input_state: InputBoxState,
    pub grid_state: EmojiGridState,
    pub selected_index: usize,
}

impl EmojiPickerState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn next(&mut self, cols: usize) {
        let count = self.filtered_emojis().len();
        if count == 0 {
            return;
        }
        if self.selected_index + cols < count {
            self.selected_index += cols;
        } else if self.selected_index + 1 < count {
            self.selected_index += 1;
        } else {
            self.selected_index = 0;
        }
    }

    pub fn previous(&mut self, cols: usize) {
        let count = self.filtered_emojis().len();
        if count == 0 {
            return;
        }
        if self.selected_index >= cols {
            self.selected_index -= cols;
        } else if self.selected_index > 0 {
            self.selected_index -= 1;
        } else {
            self.selected_index = count.saturating_sub(1);
        }
    }

    pub fn next_item(&mut self) {
        let count = self.filtered_emojis().len();
        if count == 0 {
            return;
        }
        self.selected_index = (self.selected_index + 1) % count;
    }

    pub fn previous_item(&mut self) {
        let count = self.filtered_emojis().len();
        if count == 0 {
            return;
        }
        if self.selected_index == 0 {
            self.selected_index = count.saturating_sub(1);
        } else {
            self.selected_index -= 1;
        }
    }

    pub fn filtered_emojis(&self) -> Vec<String> {
        let query = self.input_state.text.to_lowercase();
        let mut seen = std::collections::HashSet::new();
        crate::emojis::EMOJIS
            .iter()
            .filter(|(name, emoji)| {
                if query.is_empty() {
                    return seen.insert(*emoji);
                }
                if name.to_lowercase().contains(&query) {
                    return seen.insert(*emoji);
                }
                false
            })
            .map(|(_, emoji)| emoji.to_string())
            .collect()
    }

    pub fn get_selected_emoji(&self) -> Option<String> {
        let filtered = self.filtered_emojis();
        filtered.get(self.selected_index).cloned()
    }
}

pub struct EmojiPicker<'a> {
    block: Option<Block<'a>>,
    width_percent: u16,
    height_percent: u16,
}

impl<'a> Default for EmojiPicker<'a> {
    fn default() -> Self {
        Self {
            block: Some(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Emoji Picker "),
            ),
            width_percent: 50,
            height_percent: 50,
        }
    }
}

impl<'a> EmojiPicker<'a> {
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }
}

impl<'a> StatefulWidget for EmojiPicker<'a> {
    type State = EmojiPickerState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        // Dim the background
        for x in area.left()..area.right() {
            for y in area.top()..area.bottom() {
                let cell = &mut buf[(x, y)];
                cell.set_style(cell.style().add_modifier(Modifier::DIM));
            }
        }

        let modal_area = centered_rect(self.width_percent, self.height_percent, area);
        Clear.render(modal_area, buf);

        let block = self.block.unwrap_or_default();
        let inner_area = block.inner(modal_area);
        block.render(modal_area, buf);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Search box
                Constraint::Min(1),    // Grid
                Constraint::Length(1), // Hints
            ])
            .split(inner_area);

        // Search Input
        let input = InputBox::default()
            .focused(true)
            .prompt("Search: ❯ ")
            .block(Block::default().borders(Borders::BOTTOM));
        StatefulWidget::render(input, chunks[0], buf, &mut state.input_state);

        // Grid
        let emojis = state.filtered_emojis();

        // Ensure selection is valid
        if state.selected_index >= emojis.len() && !emojis.is_empty() {
            state.selected_index = 0;
        }

        let grid = EmojiGrid::new(&emojis, state.selected_index);
        StatefulWidget::render(grid, chunks[1], buf, &mut state.grid_state);

        // Hints
        let hints = "[↑↓←→] Navigate [Enter] Select [Esc] Close";
        buf.set_string(
            chunks[2].x,
            chunks[2].y,
            hints,
            Style::default().add_modifier(Modifier::DIM),
        );
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
