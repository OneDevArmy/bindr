use crate::llm::LlmEvent;
use anyhow::Result;
use ratatui::text::{Line, Span};
use std::collections::VecDeque;

/// State for managing streaming text output
#[derive(Clone)]
pub struct StreamState {
    /// Lines ready to be displayed
    queued_lines: VecDeque<Line<'static>>,
    /// Whether we've seen any content
    has_content: bool,
    /// Current line being built
    current_line: String,
}

impl StreamState {
    pub fn new() -> Self {
        Self {
            queued_lines: VecDeque::new(),
            has_content: false,
            current_line: String::new(),
        }
    }

    /// Process a text delta from the LLM
    pub fn push_delta(&mut self, delta: &str) {
        if !delta.is_empty() {
            self.has_content = true;
        }

        self.current_line.push_str(delta);

        // Check for complete lines
        while let Some(newline_pos) = self.current_line.find('\n') {
            let line_content = self.current_line[..newline_pos].to_string();
            self.current_line = self.current_line[newline_pos + 1..].to_string();

            // Create a line with the content
            let line = Line::from(vec![Span::raw(line_content)]);
            self.queued_lines.push_back(line);
        }
    }

    /// Get all remaining lines
    pub fn drain_lines(&mut self) -> Vec<Line<'static>> {
        self.queued_lines.drain(..).collect()
    }

    /// Finalize and get any remaining content
    pub fn finalize(&mut self) -> Vec<Line<'static>> {
        let mut lines = self.drain_lines();

        // Add the final partial line if it has content
        if !self.current_line.trim().is_empty() {
            lines.push(Line::from(vec![Span::raw(self.current_line.clone())]));
        }

        lines
    }

    /// Clear all state
    pub fn clear(&mut self) {
        self.queued_lines.clear();
        self.current_line.clear();
        self.has_content = false;
    }
}

/// Controller for managing streaming LLM responses
#[derive(Clone)]
pub struct StreamController {
    state: StreamState,
    is_streaming: bool,
    is_complete: bool,
}

impl StreamController {
    pub fn new() -> Self {
        Self {
            state: StreamState::new(),
            is_streaming: false,
            is_complete: false,
        }
    }

    /// Start streaming
    pub fn start_streaming(&mut self) {
        self.is_streaming = true;
        self.is_complete = false;
    }

    /// Process an LLM event
    pub fn process_event(&mut self, event: LlmEvent) -> Result<Vec<Line<'static>>> {
        match event {
            LlmEvent::TextDelta(delta) => {
                self.state.push_delta(&delta);
                Ok(self.state.drain_lines())
            }
            LlmEvent::ResponseComplete(content) => {
                self.state.push_delta(&content);
                Ok(self.state.drain_lines())
            }
            LlmEvent::ReasoningDelta(delta) => {
                // For now, treat reasoning the same as text
                // Could be styled differently in the future
                self.state.push_delta(&format!("💭 {}", delta));
                Ok(self.state.drain_lines())
            }
            LlmEvent::StreamComplete => {
                self.is_complete = true;
                self.is_streaming = false;
                Ok(self.state.finalize())
            }
            LlmEvent::Error(error) => {
                self.is_complete = true;
                self.is_streaming = false;
                let error_line = Line::from(vec![
                    Span::styled(
                        "❌ Error: ",
                        ratatui::style::Style::default().fg(ratatui::style::Color::Red),
                    ),
                    Span::raw(error),
                ]);
                Ok(vec![error_line])
            }
        }
    }

    /// Reset the controller
    pub fn reset(&mut self) {
        self.state.clear();
        self.is_streaming = false;
        self.is_complete = false;
    }
}
