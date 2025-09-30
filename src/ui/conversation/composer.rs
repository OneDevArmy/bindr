use crate::events::BindrMode;
use crate::ui::conversation::commands::SlashCommand;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Widget},
};
use std::cell::RefCell;

/// Result returned when the user interacts with the conversation composer
#[derive(Debug, PartialEq)]
pub enum ConversationResult {
    Submitted(String),
    Command(SlashCommand),
    None,
}

/// State for the text area within the composer
#[derive(Debug, Clone)]
pub struct TextAreaState {
    pub content: String,
    pub cursor_position: usize,
    #[allow(dead_code)]
    pub scroll_offset: usize,
}

impl Default for TextAreaState {
    fn default() -> Self {
        Self {
            content: String::new(),
            cursor_position: 0,
            scroll_offset: 0,
        }
    }
}

/// Conversation composer for user input
#[derive(Clone)]
pub struct ConversationComposer {
    state: RefCell<TextAreaState>,
    placeholder: String,
    has_focus: bool,
    current_mode: BindrMode,
}

impl ConversationComposer {
    pub fn new(placeholder: String, current_mode: BindrMode) -> Self {
        Self {
            state: RefCell::new(TextAreaState::default()),
            placeholder,
            has_focus: false,
            current_mode,
        }
    }

    /// Handle key input
    pub fn handle_key(&self, key: KeyEvent) -> ConversationResult {
        if key.kind != KeyEventKind::Press {
            return ConversationResult::None;
        }

        let mut state = self.state.borrow_mut();

        match key.code {
            KeyCode::Enter => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    // Shift+Enter: new line
                    self.insert_char(&mut state, '\n');
                } else {
                    // Enter: submit
                    if !state.content.trim().is_empty() {
                        let content = state.content.clone();
                        state.content.clear();
                        state.cursor_position = 0;
                        
                        // Check if it's a slash command
                        if let Some(command) = crate::ui::conversation::commands::parse_slash_command(&content) {
                            return ConversationResult::Command(command);
                        } else {
                            return ConversationResult::Submitted(content);
                        }
                    }
                }
            }
            KeyCode::Char(c) => {
                self.insert_char(&mut state, c);
            }
            KeyCode::Backspace => {
                self.backspace(&mut state);
            }
            KeyCode::Delete => {
                self.delete(&mut state);
            }
            KeyCode::Left => {
                if state.cursor_position > 0 {
                    state.cursor_position -= 1;
                }
            }
            KeyCode::Right => {
                if state.cursor_position < state.content.len() {
                    state.cursor_position += 1;
                }
            }
            KeyCode::Home => {
                state.cursor_position = 0;
            }
            KeyCode::End => {
                state.cursor_position = state.content.len();
            }
            _ => {}
        }

        ConversationResult::None
    }

    /// Insert a character at the cursor position
    fn insert_char(&self, state: &mut TextAreaState, c: char) {
        state.content.insert(state.cursor_position, c);
        state.cursor_position += 1;
    }

    /// Delete character before cursor
    fn backspace(&self, state: &mut TextAreaState) {
        if state.cursor_position > 0 {
            state.cursor_position -= 1;
            state.content.remove(state.cursor_position);
        }
    }

    /// Delete character at cursor
    fn delete(&self, state: &mut TextAreaState) {
        if state.cursor_position < state.content.len() {
            state.content.remove(state.cursor_position);
        }
    }

    /// Set focus state
    pub fn set_focus(&mut self, has_focus: bool) {
        self.has_focus = has_focus;
    }

    /// Update current mode
    #[allow(dead_code)]
    pub fn update_mode(&mut self, mode: BindrMode) {
        self.current_mode = mode;
    }

    /// Get current content
    #[allow(dead_code)]
    pub fn get_content(&self) -> String {
        self.state.borrow().content.clone()
    }

    /// Clear content
    #[allow(dead_code)]
    pub fn clear(&self) {
        let mut state = self.state.borrow_mut();
        state.content.clear();
        state.cursor_position = 0;
    }
}

impl Widget for ConversationComposer {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let state = self.state.borrow();
        
        // Create the input block
        let block = Block::default()
            .borders(Borders::ALL)
            .title(self.get_mode_title())
            .style(if self.has_focus {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Gray)
            });

        let inner_area = block.inner(area);
        block.render(area, buf);

        // Render content or placeholder
        if state.content.is_empty() {
            let placeholder_line = Line::from(vec![
                Span::styled(
                    &self.placeholder,
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            buf.set_line(inner_area.x, inner_area.y, &placeholder_line, inner_area.width);
        } else {
            // Render content with cursor
            let lines: Vec<&str> = state.content.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                if i < inner_area.height as usize {
                    let line_span = Span::raw(*line);
                    let line = Line::from(vec![line_span]);
                    buf.set_line(inner_area.x, inner_area.y + i as u16, &line, inner_area.width);
                }
            }
        }
    }
}

impl ConversationComposer {
    /// Get mode-specific title
    fn get_mode_title(&self) -> String {
        match self.current_mode {
            BindrMode::Brainstorm => "💡 Brainstorm - Share your ideas",
            BindrMode::Plan => "📋 Plan - Describe your project",
            BindrMode::Execute => "⚡ Execute - What should I build?",
            BindrMode::Document => "📝 Document - What should I document?",
        }
        .to_string()
    }
}
