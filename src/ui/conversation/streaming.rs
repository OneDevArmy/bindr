use crate::events::{BindrMode, LlmStreamEvent};
use crate::llm::LlmEvent;
use crate::streaming::StreamController;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Widget,
};
use std::collections::VecDeque;

/// Streaming response display component
#[derive(Clone)]
pub struct StreamingResponse {
    controller: StreamController,
    current_response: String,
    is_streaming: bool,
    mode: BindrMode,
    response_lines: VecDeque<Line<'static>>,
}

impl StreamingResponse {
    pub fn new(mode: BindrMode) -> Self {
        Self {
            controller: StreamController::new(),
            current_response: String::new(),
            is_streaming: false,
            mode,
            response_lines: VecDeque::new(),
        }
    }

    /// Start streaming a new response
    pub fn start_streaming(&mut self) {
        self.is_streaming = true;
        self.current_response.clear();
        self.response_lines.clear();
        self.controller.reset();
        self.controller.start_streaming();
    }

    /// Process a streaming event
    pub fn process_event(&mut self, event: LlmStreamEvent) -> bool {
        match event {
            LlmStreamEvent::TextDelta(delta) => {
                self.current_response.push_str(&delta);
                let llm_event = LlmEvent::TextDelta(delta);
                self.controller.process_event(llm_event).unwrap_or_default();
                true
            }
            LlmStreamEvent::ResponseComplete(content) => {
                self.current_response = content.clone();
                let llm_event = LlmEvent::ResponseComplete(content);
                self.controller.process_event(llm_event).unwrap_or_default();
                true
            }
            LlmStreamEvent::ReasoningDelta(delta) => {
                // Handle reasoning content
                let llm_event = LlmEvent::ReasoningDelta(delta);
                self.controller.process_event(llm_event).unwrap_or_default();
                true
            }
            LlmStreamEvent::StreamComplete => {
                self.is_streaming = false;
                let llm_event = LlmEvent::StreamComplete;
                self.controller.process_event(llm_event).unwrap_or_default();
                false // Streaming complete
            }
            LlmStreamEvent::Error(error) => {
                self.is_streaming = false;
                self.add_error_line(&error);
                false
            }
        }
    }

    /// Add an error line to the response
    fn add_error_line(&mut self, error: &str) {
        let error_line = Line::from(vec![
            Span::styled("❌ Error: ", Style::default().fg(Color::Red)),
            Span::raw(error.to_string()),
        ]);
        self.response_lines.push_back(error_line);
    }

    /// Check if currently streaming
    pub fn is_streaming(&self) -> bool {
        self.is_streaming
    }

    /// Get the current response content
    pub fn get_response(&self) -> String {
        self.current_response.clone()
    }

    /// Clear the response
    pub fn clear(&mut self) {
        self.current_response.clear();
        self.response_lines.clear();
        self.is_streaming = false;
        self.controller.reset();
    }

    /// Update the mode
    pub fn update_mode(&mut self, mode: BindrMode) {
        self.mode = mode;
    }
}

impl From<LlmEvent> for LlmStreamEvent {
    fn from(event: LlmEvent) -> Self {
        match event {
            LlmEvent::TextDelta(delta) => LlmStreamEvent::TextDelta(delta),
            LlmEvent::ResponseComplete(content) => LlmStreamEvent::ResponseComplete(content),
            LlmEvent::ReasoningDelta(reasoning) => LlmStreamEvent::ReasoningDelta(reasoning),
            LlmEvent::StreamComplete => LlmStreamEvent::StreamComplete,
            LlmEvent::Error(error) => LlmStreamEvent::Error(error),
        }
    }
}

impl Widget for StreamingResponse {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.is_streaming && self.current_response.is_empty() {
            return;
        }

        let mut y_offset = 0;

        // Render streaming indicator with animated dots
        if self.is_streaming {
            let dots = match (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                / 300)
                % 4
            {
                0 => ".",
                1 => "..",
                2 => "...",
                _ => "   ",
            };

            let indicator = Line::from(vec![
                Span::styled("🤖 ", Style::default().fg(Color::Green)),
                Span::styled("Bindr is thinking", Style::default().fg(Color::Green)),
                Span::styled(dots, Style::default().fg(Color::Yellow)),
            ]);
            buf.set_line(area.x, area.y + y_offset, &indicator, area.width);
            y_offset += 1;
        }

        // Render current response content
        if !self.current_response.is_empty() {
            let content_lines = self.wrap_text(
                &self.current_response,
                area.width.saturating_sub(2) as usize,
            );
            for line in content_lines {
                if y_offset < area.height {
                    let response_line = Line::from(vec![
                        Span::raw("  "),
                        Span::styled(line, Style::default().fg(Color::Green)),
                    ]);
                    buf.set_line(area.x, area.y + y_offset as u16, &response_line, area.width);
                    y_offset += 1;
                }
            }
        }

        // Render blinking cursor if streaming
        if self.is_streaming && y_offset < area.height {
            let cursor_char = if (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                / 500)
                % 2
                == 0
            {
                "▋"
            } else {
                " "
            };

            let cursor_line = Line::from(vec![
                Span::raw("  "),
                Span::styled(cursor_char, Style::default().fg(Color::Green)),
            ]);
            buf.set_line(area.x, area.y + y_offset as u16, &cursor_line, area.width);
        }
    }
}

impl StreamingResponse {
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
}
