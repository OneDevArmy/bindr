use crate::llm::LlmEvent;
use anyhow::Result;
use ratatui::text::{Line, Span};
use std::collections::VecDeque;
use tokio::sync::mpsc;

/// State for managing streaming text output
#[derive(Clone)]
pub struct StreamState {
    /// Accumulated text buffer
    text_buffer: String,
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
            text_buffer: String::new(),
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
        
        self.text_buffer.push_str(delta);
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

    /// Get the next line to display
    #[allow(dead_code)]
    pub fn pop_line(&mut self) -> Option<Line<'static>> {
        self.queued_lines.pop_front()
    }

    /// Get all remaining lines
    pub fn drain_lines(&mut self) -> Vec<Line<'static>> {
        self.queued_lines.drain(..).collect()
    }

    /// Check if there are lines ready to display
    #[allow(dead_code)]
    pub fn has_lines(&self) -> bool {
        !self.queued_lines.is_empty()
    }

    /// Get the current partial line (for cursor display)
    #[allow(dead_code)]
    pub fn get_current_line(&self) -> &str {
        &self.current_line
    }

    /// Check if we have any content
    #[allow(dead_code)]
    pub fn has_content(&self) -> bool {
        self.has_content
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
        self.text_buffer.clear();
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
                    Span::styled("❌ Error: ", ratatui::style::Style::default().fg(ratatui::style::Color::Red)),
                    Span::raw(error),
                ]);
                Ok(vec![error_line])
            }
        }
    }

    /// Get the current partial line for display
    #[allow(dead_code)]
    pub fn get_current_line(&self) -> Option<Line<'static>> {
        let current = self.state.get_current_line();
        if current.is_empty() {
            None
        } else {
            Some(Line::from(vec![
                Span::raw(current.to_string()),
                Span::styled("▋", ratatui::style::Style::default().fg(ratatui::style::Color::Green)),
            ]))
        }
    }

    /// Check if streaming is active
    #[allow(dead_code)]
    pub fn is_streaming(&self) -> bool {
        self.is_streaming
    }

    /// Check if streaming is complete
    #[allow(dead_code)]
    pub fn is_complete(&self) -> bool {
        self.is_complete
    }

    /// Check if we have any content
    #[allow(dead_code)]
    pub fn has_content(&self) -> bool {
        self.state.has_content()
    }

    /// Reset the controller
    pub fn reset(&mut self) {
        self.state.clear();
        self.is_streaming = false;
        self.is_complete = false;
    }
}

/// Stream processor that handles LLM events and updates the UI
#[allow(dead_code)]
pub struct StreamProcessor {
    controller: StreamController,
    event_rx: mpsc::Receiver<LlmEvent>,
}

impl StreamProcessor {
    #[allow(dead_code)]
    pub fn new(event_rx: mpsc::Receiver<LlmEvent>) -> Self {
        Self {
            controller: StreamController::new(),
            event_rx,
        }
    }

    /// Process the next event
    #[allow(dead_code)]
    pub async fn process_next(&mut self) -> Result<Option<Vec<Line<'static>>>> {
        if let Some(event) = self.event_rx.recv().await {
            if !self.controller.is_streaming() {
                self.controller.start_streaming();
            }
            Ok(Some(self.controller.process_event(event)?))
        } else {
            Ok(None)
        }
    }

    /// Get the current partial line
    #[allow(dead_code)]
    pub fn get_current_line(&self) -> Option<Line<'static>> {
        self.controller.get_current_line()
    }

    /// Check if streaming is complete
    #[allow(dead_code)]
    pub fn is_complete(&self) -> bool {
        self.controller.is_complete()
    }

    /// Check if we have content
    #[allow(dead_code)]
    pub fn has_content(&self) -> bool {
        self.controller.has_content()
    }

    /// Reset the processor
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.controller.reset();
    }
}

/// Helper to create styled lines for different message types
#[allow(dead_code)]
pub fn create_message_line(content: &str, role: &str) -> Line<'static> {
    match role {
        "user" => Line::from(vec![
            Span::styled("👤 You: ", ratatui::style::Style::default().fg(ratatui::style::Color::Blue)),
            Span::raw(content.to_string()),
        ]),
        "assistant" => Line::from(vec![
            Span::styled("🤖 Bindr: ", ratatui::style::Style::default().fg(ratatui::style::Color::Green)),
            Span::raw(content.to_string()),
        ]),
        "system" => Line::from(vec![
            Span::styled("⚙️ System: ", ratatui::style::Style::default().fg(ratatui::style::Color::Yellow)),
            Span::raw(content.to_string()),
        ]),
        _ => Line::from(vec![Span::raw(content.to_string())]),
    }
}

/// Helper to create status lines
#[allow(dead_code)]
pub fn create_status_line(message: &str, status: StatusType) -> Line<'static> {
    match status {
        StatusType::Info => Line::from(vec![
            Span::styled("ℹ️ ", ratatui::style::Style::default().fg(ratatui::style::Color::Blue)),
            Span::raw(message.to_string()),
        ]),
        StatusType::Success => Line::from(vec![
            Span::styled("✅ ", ratatui::style::Style::default().fg(ratatui::style::Color::Green)),
            Span::raw(message.to_string()),
        ]),
        StatusType::Warning => Line::from(vec![
            Span::styled("⚠️ ", ratatui::style::Style::default().fg(ratatui::style::Color::Yellow)),
            Span::raw(message.to_string()),
        ]),
        StatusType::Error => Line::from(vec![
            Span::styled("❌ ", ratatui::style::Style::default().fg(ratatui::style::Color::Red)),
            Span::raw(message.to_string()),
        ]),
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum StatusType {
    Info,
    Success,
    Warning,
    Error,
}
