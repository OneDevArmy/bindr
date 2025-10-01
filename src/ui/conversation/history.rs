//! Conversation history display component

use crate::events::{BindrMode, ConversationRole};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Scrollbar, ScrollbarOrientation, ScrollbarState, Widget},
};
use std::collections::VecDeque;

/// A single message in the conversation history
#[derive(Debug, Clone)]
pub struct ConversationMessage {
    pub role: ConversationRole,
    pub content: String,
    pub mode: BindrMode,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Conversation history display component
#[derive(Clone)]
pub struct ConversationHistory {
    messages: VecDeque<ConversationMessage>,
    #[allow(dead_code)]
    scroll_state: ScrollbarState,
    max_messages: usize,
    streaming_message: Option<String>,
}

impl ConversationHistory {
    pub fn new(max_messages: usize) -> Self {
        Self {
            messages: VecDeque::new(),
            scroll_state: ScrollbarState::default(),
            max_messages,
            streaming_message: None,
        }
    }

    /// Add a new message to the history
    pub fn add_message(&mut self, message: ConversationMessage) {
        self.messages.push_back(message);
        
        // Limit message count
        if self.messages.len() > self.max_messages {
            self.messages.pop_front();
        }
        
        // Auto-scroll to bottom
        self.scroll_to_bottom();
    }

    /// Add a user message
    pub fn add_user_message(&mut self, content: String, mode: BindrMode) {
        let message = ConversationMessage {
            role: ConversationRole::User,
            content,
            mode,
            timestamp: chrono::Utc::now(),
        };
        self.add_message(message);
    }

    /// Add an assistant message
    pub fn add_assistant_message(&mut self, content: String, mode: BindrMode) {
        let message = ConversationMessage {
            role: ConversationRole::Assistant,
            content,
            mode,
            timestamp: chrono::Utc::now(),
        };
        self.add_message(message);
    }

    /// Add a system message
    pub fn add_system_message(&mut self, content: String, mode: BindrMode) {
        let message = ConversationMessage {
            role: ConversationRole::System,
            content,
            mode,
            timestamp: chrono::Utc::now(),
        };
        self.add_message(message);
    }

    /// Scroll up
    #[allow(dead_code)]
    pub fn scroll_up(&mut self) {
        // TODO: Implement proper scrolling
    }

    /// Scroll down
    #[allow(dead_code)]
    pub fn scroll_down(&mut self) {
        // TODO: Implement proper scrolling
    }

    /// Scroll to bottom
    pub fn scroll_to_bottom(&mut self) {
        // TODO: Implement proper scrolling
    }

    /// Clear all messages
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.messages.clear();
        self.scroll_state = ScrollbarState::default();
    }

    /// Get message count
    #[allow(dead_code)]
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Set the current streaming message
    pub fn set_streaming_message(&mut self, message: String) {
        self.streaming_message = Some(message);
    }

    /// Clear the streaming message
    pub fn clear_streaming_message(&mut self) {
        self.streaming_message = None;
    }
}

impl Widget for ConversationHistory {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title("💬 Conversation History");

        let inner_area = block.inner(area);
        block.render(area, buf);

        if self.messages.is_empty() {
            // Show welcome message
            let welcome_lines = vec![
                Line::from(vec![Span::styled("Welcome to Bindr! 🚀", Style::default().fg(Color::Green))]),
                Line::from(vec![Span::raw("")]),
                Line::from(vec![Span::styled("Start by sharing your ideas below.", Style::default().fg(Color::Gray))]),
                Line::from(vec![Span::raw("")]),
                Line::from(vec![Span::styled("Press Enter to send, Shift+Enter for new line.", Style::default().fg(Color::DarkGray))]),
            ];

            for (i, line) in welcome_lines.iter().enumerate() {
                if i < inner_area.height as usize {
                    buf.set_line(inner_area.x, inner_area.y + i as u16, line, inner_area.width);
                }
            }
        } else {
            // Collect all lines for messages (including streaming if any)
            let mut all_lines: Vec<Line> = Vec::new();
            for message in self.messages.iter() {
                let mut lines = self.render_message(message, inner_area.width);
                all_lines.append(&mut lines);
                // spacing between messages
                all_lines.push(Line::from(vec![Span::raw("")]))
            }

            if let Some(ref streaming_text) = self.streaming_message {
                let mut streaming_lines = self.render_streaming_message(streaming_text, inner_area.width);
                all_lines.append(&mut streaming_lines);
            }

            // Determine the range of lines to display from the bottom
            let height = inner_area.height as usize;
            let total = all_lines.len();
            let start = total.saturating_sub(height);
            let visible = &all_lines[start..];

            for (i, line) in visible.iter().enumerate() {
                buf.set_line(inner_area.x, inner_area.y + i as u16, line, inner_area.width);
            }
        }

        // Render scrollbar placeholder
        let _scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"));
    }
}

impl ConversationHistory {
    /// Render a single message into lines
    fn render_message(&self, message: &ConversationMessage, width: u16) -> Vec<Line> {
        let mut lines = Vec::new();
        
        // Message header with role and timestamp
        let role_icon = match message.role {
            ConversationRole::User => "👤",
            ConversationRole::Assistant => "🤖",
            ConversationRole::System => "⚙️",
        };
        
        let mode_text = match message.mode {
            BindrMode::Brainstorm => "💡",
            BindrMode::Plan => "📋",
            BindrMode::Execute => "⚡",
            BindrMode::Document => "📝",
        };
        
        let timestamp = message.timestamp.format("%H:%M:%S").to_string();
        let header = format!("{} {} {} {}", role_icon, mode_text, timestamp, "─".repeat(20));
        
        lines.push(Line::from(vec![
            Span::styled(header, Style::default().fg(Color::DarkGray)),
        ]));
        
        // Message content
        let content_lines = self.wrap_text(&message.content, width.saturating_sub(2) as usize);
        for content_line in content_lines {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(content_line, self.get_content_style(&message.role)),
            ]));
        }
        
        lines
    }

    /// Wrap text to fit within the given width
    fn wrap_text(&self, text: &str, width: usize) -> Vec<String> {
        if width == 0 {
            return vec![text.to_string()];
        }
        
        let mut lines = Vec::new();
        let mut current_line = String::new();
        
        for word in text.split_whitespace() {
            if current_line.len() + word.len() + 1 <= width {
                if !current_line.is_empty() {
                    current_line.push(' ');
                }
                current_line.push_str(word);
            } else {
                if !current_line.is_empty() {
                    lines.push(current_line);
                    current_line = String::new();
                }
                current_line.push_str(word);
            }
        }
        
        if !current_line.is_empty() {
            lines.push(current_line);
        }
        
        if lines.is_empty() {
            lines.push(String::new());
        }
        
        lines
    }

    /// Get content style based on role
    fn get_content_style(&self, role: &ConversationRole) -> Style {
        match role {
            ConversationRole::User => Style::default().fg(Color::Blue),
            ConversationRole::Assistant => Style::default().fg(Color::Green),
            ConversationRole::System => Style::default().fg(Color::Yellow),
        }
    }

    /// Render a streaming message with typing indicator
    fn render_streaming_message(&self, text: &str, width: u16) -> Vec<Line> {
        let mut lines = Vec::new();
        
        // Streaming message header
        let timestamp = chrono::Utc::now().format("%H:%M:%S").to_string();
        let header = format!("🤖 💡 {} {}", timestamp, "─".repeat(20));
        
        lines.push(Line::from(vec![
            Span::styled(header, Style::default().fg(Color::DarkGray)),
        ]));
        
        // Streaming content with cursor
        let content_lines = self.wrap_text(text, width.saturating_sub(2) as usize);
        for (i, content_line) in content_lines.iter().enumerate() {
            let is_last_line = i == content_lines.len() - 1;
            let cursor = if is_last_line { "▋" } else { "" };
            
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(content_line.clone(), Style::default().fg(Color::Green)),
                Span::styled(cursor, Style::default().fg(Color::Yellow)),
            ]));
        }
        
        lines
    }
}
