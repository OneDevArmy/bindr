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
}

impl ConversationHistory {
    pub fn new(max_messages: usize) -> Self {
        Self {
            messages: VecDeque::new(),
            scroll_state: ScrollbarState::default(),
            max_messages,
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
            // Render messages
            let mut y_offset = 0;
            let start_idx = 0; // TODO: Implement proper scrolling
            
            for (_i, message) in self.messages.iter().enumerate().skip(start_idx) {
                if y_offset >= inner_area.height as usize {
                    break;
                }

                // Render message
                let message_lines = self.render_message(message, inner_area.width);
                for line in message_lines {
                    if y_offset < inner_area.height as usize {
                        buf.set_line(inner_area.x, inner_area.y + y_offset as u16, &line, inner_area.width);
                        y_offset += 1;
                    }
                }
                
                // Add spacing between messages
                if y_offset < inner_area.height as usize {
                    y_offset += 1;
                }
            }
        }

        // Render scrollbar
        let _scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"));
        
        // Note: Scrollbar rendering is complex in ratatui, for now we'll skip it
        // TODO: Implement proper scrollbar rendering
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
}
