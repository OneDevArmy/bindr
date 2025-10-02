use crate::events::BindrMode;
use crate::ui::conversation::commands::{command_entries, CommandEntry, ParsedCommand};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Widget},
};
use std::cell::{Cell, RefCell};

/// Result returned when the user interacts with the conversation composer
#[derive(Debug, PartialEq)]
pub enum ConversationResult {
    Submitted(String),
    Command(ParsedCommand),
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
    command_entries: Vec<CommandEntry>,
    filtered_commands: RefCell<Vec<CommandEntry>>,
    show_command_palette: Cell<bool>,
    selected_command: Cell<Option<usize>>,
}

impl ConversationComposer {
    pub fn new(placeholder: String, current_mode: BindrMode) -> Self {
        Self {
            state: RefCell::new(TextAreaState::default()),
            placeholder,
            has_focus: false,
            current_mode,
            command_entries: command_entries(),
            filtered_commands: RefCell::new(Vec::new()),
            show_command_palette: Cell::new(false),
            selected_command: Cell::new(None),
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
                    self.insert_char(&mut state, '\n');
                } else if self.show_command_palette.get() {
                    if self.apply_selected_command(&mut state) {
                        return ConversationResult::None;
                    }
                } else if !state.content.trim().is_empty() {
                    let content = state.content.clone();
                    state.content.clear();
                    state.cursor_position = 0;
                    self.close_command_palette();
                    drop(state);
                    if let Some(command) = crate::ui::conversation::commands::parse_slash_command(&content) {
                        return ConversationResult::Command(command);
                    } else {
                        return ConversationResult::Submitted(content);
                    }
                }
            }
            KeyCode::Up => {
                if self.show_command_palette.get() {
                    self.move_command_selection(-1);
                    return ConversationResult::None;
                }
            }
            KeyCode::Down => {
                if self.show_command_palette.get() {
                    self.move_command_selection(1);
                    return ConversationResult::None;
                }
            }
            KeyCode::Esc => {
                if self.show_command_palette.get() {
                    self.close_command_palette();
                    return ConversationResult::None;
                }
            }
            KeyCode::Tab => {
                if self.show_command_palette.get() {
                    if self.apply_selected_command(&mut state) {
                        return ConversationResult::None;
                    }
                }
            }
            KeyCode::Char(c) => {
                if c == '/' && state.content.is_empty() {
                    self.insert_char(&mut state, c);
                    self.open_command_palette(&state);
                    return ConversationResult::None;
                }

                self.insert_char(&mut state, c);

                if self.show_command_palette.get() {
                    if state.content.starts_with('/') {
                        if c.is_whitespace() {
                            self.close_command_palette();
                        } else {
                            self.refresh_command_palette(&state);
                        }
                    } else {
                        self.close_command_palette();
                    }
                } else if state.content == "/" {
                    self.open_command_palette(&state);
                }
            }
            KeyCode::Backspace => {
                if self.backspace(&mut state) {
                    if self.show_command_palette.get() {
                        if state.content.starts_with('/') {
                            self.refresh_command_palette(&state);
                        } else {
                            self.close_command_palette();
                        }
                    }
                }
            }
            KeyCode::Delete => {
                if self.delete(&mut state) {
                    if self.show_command_palette.get() {
                        if state.content.starts_with('/') {
                            self.refresh_command_palette(&state);
                        } else {
                            self.close_command_palette();
                        }
                    }
                }
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
    fn backspace(&self, state: &mut TextAreaState) -> bool {
        if state.cursor_position > 0 {
            state.cursor_position -= 1;
            state.content.remove(state.cursor_position);
            true
        } else {
            false
        }
    }

    /// Delete character at cursor
    fn delete(&self, state: &mut TextAreaState) -> bool {
        if state.cursor_position < state.content.len() {
            state.content.remove(state.cursor_position);
            true
        } else {
            false
        }
    }

    fn open_command_palette(&self, state: &TextAreaState) {
        self.show_command_palette.set(true);
        self.refresh_command_palette(state);
        self.selected_command.set(Some(0));
    }

    fn close_command_palette(&self) {
        self.show_command_palette.set(false);
        self.filtered_commands.borrow_mut().clear();
        self.selected_command.set(None);
    }

    fn refresh_command_palette(&self, state: &TextAreaState) {
        let query = state.content.trim_start_matches('/').to_lowercase();
        let mut filtered = self.filtered_commands.borrow_mut();
        filtered.clear();

        for entry in &self.command_entries {
            if query.is_empty() || entry.keyword.starts_with(&query) {
                filtered.push(*entry);
            }
        }

        if filtered.is_empty() {
            self.selected_command.set(None);
        } else {
            let index = self.selected_command.get().unwrap_or(0);
            let clamped = index.min(filtered.len() - 1);
            self.selected_command.set(Some(clamped));
        }
    }

    fn move_command_selection(&self, delta: isize) {
        let filtered = self.filtered_commands.borrow();
        if filtered.is_empty() {
            self.selected_command.set(None);
            return;
        }

        let current = self.selected_command.get().unwrap_or(0) as isize;
        let len = filtered.len() as isize;
        let mut next = current + delta;

        if next < 0 {
            next = len - 1;
        } else if next >= len {
            next = 0;
        }

        self.selected_command.set(Some(next as usize));
    }

    fn apply_selected_command(&self, state: &mut TextAreaState) -> bool {
        let filtered = self.filtered_commands.borrow();
        let Some(index) = self.selected_command.get() else {
            return false;
        };

        if index >= filtered.len() {
            return false;
        }

        let entry = filtered[index];
        state.content = format!("/{} ", entry.keyword);
        state.cursor_position = state.content.len();
        drop(filtered);
        self.close_command_palette();
        self.refresh_command_palette(state);
        true
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
            // Render content with cursor indicator
            let mut content = state.content.clone();
            if self.has_focus {
                content.insert(state.cursor_position.min(content.len()), '▌');
            }

            for (i, line_text) in content.split('\n').enumerate() {
                if i < inner_area.height as usize {
                    let line = Line::from(vec![Span::raw(line_text)]);
                    buf.set_line(inner_area.x, inner_area.y + i as u16, &line, inner_area.width);
                }
            }
        }

        // Render command palette if active
        if self.show_command_palette.get() {
            let filtered = self.filtered_commands.borrow();
            let palette_height = (filtered.len().min(5) + 2) as u16;
            let palette_area = Rect {
                x: inner_area.x,
                y: inner_area.y.saturating_sub(palette_height).max(0),
                width: inner_area.width,
                height: palette_height,
            };

            let block = Block::default()
                .borders(Borders::ALL)
                .title("Commands")
                .style(Style::default().fg(Color::Blue));
            let inner = block.inner(palette_area);
            block.render(palette_area, buf);

            let selected = self.selected_command.get();
            for (index, entry) in filtered.iter().enumerate() {
                if index >= inner.height as usize {
                    break;
                }

                let is_selected = selected == Some(index);
                let style = if is_selected {
                    Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                let line = Line::from(vec![
                    Span::styled(format!("/{}", entry.keyword), style),
                    Span::styled(" — ", Style::default().fg(Color::DarkGray)),
                    Span::styled(entry.description, Style::default().fg(Color::Gray)),
                ]);

                buf.set_line(inner.x, inner.y + index as u16, &line, inner.width);
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
