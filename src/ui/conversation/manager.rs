use crate::agent::AgentManager;
use crate::config::Config;
use crate::events::BindrMode;
use crate::llm::LlmClient;
use crate::ui::conversation::{ConversationComposer, ConversationHistory, StreamingResponse, SlashCommand, ParsedCommand, get_help_text};
use anyhow::Result;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect, Direction},
    widgets::Widget,
};
use tokio::sync::mpsc;

/// Actions that can be requested by the conversation manager
#[derive(Debug, Clone)]
pub enum ConversationAction {
    None,
    GoHome,
    Exit,
    ShowModelSelection,
}

/// Manages the conversation flow and UI components
pub struct ConversationManager {
    history: ConversationHistory,
    composer: ConversationComposer,
    streaming: StreamingResponse,
    agent_manager: AgentManager,
    #[allow(dead_code)]
    llm_client: LlmClient,
    current_mode: BindrMode,
    is_active: bool,
    stream_receiver: Option<mpsc::UnboundedReceiver<String>>,
    current_streaming_message: String,
}

impl ConversationManager {
    pub fn new(agent_manager: AgentManager, llm_client: LlmClient, mode: BindrMode) -> Self {
        let placeholder = Self::get_mode_placeholder(mode);
        
        Self {
            history: ConversationHistory::new(100),
            composer: ConversationComposer::new(placeholder, mode),
            streaming: StreamingResponse::new(mode),
            agent_manager,
            llm_client,
            current_mode: mode,
            is_active: false,
            stream_receiver: None,
            current_streaming_message: String::new(),
        }
    }

    /// Start a new conversation
    pub fn start_conversation(&mut self) {
        self.is_active = true;
        self.composer.set_focus(true);
        self.history.add_system_message(
            format!("Started {} mode", self.current_mode.display_name()),
            self.current_mode,
        );
    }

    /// Handle user input and start streaming response
    pub async fn handle_input(&mut self, input: String) -> Result<()> {
        if input.trim().is_empty() {
            return Ok(());
        }

        // Add user message to history
        self.history.add_user_message(input.clone(), self.current_mode);

        // Start streaming response
        self.streaming.start_streaming();
        self.current_streaming_message.clear();

        // Get streaming response from agent and store the receiver
        let stream_rx = self.agent_manager
            .orchestrator_mut()
            .continue_conversation(input)
            .await?;

        // Store the stream receiver for processing in the main loop
        self.stream_receiver = Some(stream_rx);

        Ok(())
    }

    /// Process streaming chunks (called from main loop)
    pub fn process_streaming_chunks(&mut self) {
        if let Some(ref mut stream_rx) = self.stream_receiver {
            loop {
                match stream_rx.try_recv() {
                    Ok(chunk) => {
                        self.current_streaming_message.push_str(&chunk);
                        // Update the streaming message in history as it grows
                        self.history.set_streaming_message(self.current_streaming_message.clone());
                    }
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                        // No more chunks right now
                        break;
                    }
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                        // Drain any remaining buffered chunks before finalizing
                        while let Ok(chunk) = stream_rx.try_recv() {
                            self.current_streaming_message.push_str(&chunk);
                        }
                        // Stream complete - finalize message
                        if !self.current_streaming_message.is_empty() {
                            self.history.add_assistant_message(
                                self.current_streaming_message.clone(),
                                self.current_mode,
                            );
                        }
                        self.history.clear_streaming_message();
                        self.current_streaming_message.clear();
                        self.stream_receiver = None;
                        self.streaming.clear();
                        break;
                    }
                }
            }
        }
    }

    /// Switch to a different mode
    pub async fn switch_mode(&mut self, new_mode: BindrMode) -> Result<()> {
        if new_mode == self.current_mode {
            return Ok(());
        }

        // Switch agent mode
        self.agent_manager.orchestrator_mut().switch_mode(new_mode).await?;

        // Update UI components
        self.current_mode = new_mode;
        let placeholder = Self::get_mode_placeholder(new_mode);
        self.composer = ConversationComposer::new(placeholder, new_mode);
        self.streaming.update_mode(new_mode);

        // Add mode switch message
        self.history.add_system_message(
            format!("Switched to {} mode", new_mode.display_name()),
            new_mode,
        );

        Ok(())
    }

    /// Handle key input
    pub async fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Result<ConversationAction> {
        match self.composer.handle_key(key) {
            crate::ui::conversation::composer::ConversationResult::Submitted(input) => {
                self.handle_input(input).await?;
                Ok(ConversationAction::None)
            }
            crate::ui::conversation::composer::ConversationResult::Command(command) => {
                self.handle_slash_command(command).await
            }
            crate::ui::conversation::composer::ConversationResult::None => {
                Ok(ConversationAction::None)
            }
        }
    }

    /// Set focus state
    pub fn set_focus(&mut self, has_focus: bool) {
        self.composer.set_focus(has_focus);
    }

    /// Check if conversation is active
    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// Get current mode
    #[allow(dead_code)]
    pub fn current_mode(&self) -> BindrMode {
        self.current_mode
    }

    /// Clear conversation
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.history.clear();
        self.composer.clear();
        self.streaming.clear();
    }

    /// Refresh configuration for agent and client
    pub fn update_config(&mut self, config: Config) {
        self.agent_manager.update_config(config.clone());
        self.llm_client = LlmClient::new(config);
    }

    /// Handle slash commands
    async fn handle_slash_command(&mut self, command: ParsedCommand) -> Result<ConversationAction> {
        match command.command {
            SlashCommand::Mode => {
                if let Some(target_mode) = command.mode_target() {
                    self.switch_mode(target_mode).await?;
                    return Ok(ConversationAction::None);
                }

                let next_mode = match self.current_mode {
                    BindrMode::Brainstorm => BindrMode::Plan,
                    BindrMode::Plan => BindrMode::Execute,
                    BindrMode::Execute => BindrMode::Document,
                    BindrMode::Document => BindrMode::Brainstorm,
                };
                self.switch_mode(next_mode).await?;
                Ok(ConversationAction::None)
            }
            SlashCommand::Home => {
                Ok(ConversationAction::GoHome)
            }
            SlashCommand::Bye => {
                Ok(ConversationAction::Exit)
            }
            SlashCommand::Help => {
                let help_text = get_help_text();
                self.history.add_system_message(help_text, self.current_mode);
                Ok(ConversationAction::None)
            }
            SlashCommand::Model => {
                Ok(ConversationAction::ShowModelSelection)
            }
        }
    }

    /// Get mode-specific placeholder text
    fn get_mode_placeholder(mode: BindrMode) -> String {
        match mode {
            BindrMode::Brainstorm => "Share your ideas and let's explore possibilities...".to_string(),
            BindrMode::Plan => "Describe your project and let's create a detailed plan...".to_string(),
            BindrMode::Execute => "What should I build? Describe the implementation...".to_string(),
            BindrMode::Document => "What should I document? Describe the documentation needs...".to_string(),
        }
    }
}

impl Widget for ConversationManager {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.is_active {
            return;
        }

        // Create layout: history takes most space, composer at bottom
        let chunks = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                Constraint::Min(10), // History
                Constraint::Length(3), // Composer
            ])
            .split(area);

        // Render history
        self.history.render(chunks[0], buf);

        // Render composer
        self.composer.render(chunks[1], buf);

        // Render streaming response if active
        if self.streaming.is_streaming() {
            // Create a small area for streaming indicator
            let streaming_area = Rect {
                x: chunks[1].x,
                y: chunks[1].y - 1,
                width: chunks[1].width,
                height: 1,
            };
            self.streaming.render(streaming_area, buf);
        }
    }
}

impl ConversationManager {
    /// Render the conversation UI components
    pub fn render_conversation_ui(&mut self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        // Create layout for conversation UI
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(10), // History area
                Constraint::Length(3), // Composer area
            ])
            .split(area);

        // Render history (includes streaming message if active)
        self.history.clone().render(chunks[0], buf);

        // Render composer
        self.composer.clone().render(chunks[1], buf);
    }

}
