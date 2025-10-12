use crate::agent::AgentManager;
use crate::config::Config;
use crate::core::intent::{ConversationContext, IntentClassifier};
use crate::core::kernel::{AgentLifecycle, IntentState, KernelEvent};
use crate::core::model_router::{ModelRequest, ModelSelector};
use crate::events::{BindrMode, LlmStreamEvent};
use crate::llm::LlmEvent;
use crate::tools::{BindrTool, ToolInvocation};
use crate::ui::conversation::{
    ConversationComposer, ConversationHistory, ParsedCommand, SlashCommand, StreamingResponse,
    get_help_text,
};
use anyhow::Result;
use chrono::Utc;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Widget},
};
use tokio::sync::broadcast::error::TryRecvError;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, info};
use uuid::Uuid;

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
    intent_router: IntentClassifier,
    model_selector: ModelSelector,
    current_mode: BindrMode,
    is_active: bool,
    stream_receiver: Option<mpsc::UnboundedReceiver<LlmEvent>>,
    kernel_events: Option<broadcast::Receiver<KernelEvent>>,
    activity_log: Vec<ActivityEntry>,
    pending_tools: Vec<PendingToolItem>,
    active_provider: String,
    active_model: String,
}

#[derive(Clone)]
struct ActivityEntry {
    timestamp: chrono::DateTime<chrono::Utc>,
    message: String,
}

struct PendingToolItem {
    id: Uuid,
    summary: String,
}

impl ConversationManager {
    pub async fn new(agent_manager: AgentManager, mode: BindrMode) -> Result<Self> {
        let placeholder = Self::get_mode_placeholder(mode);
        let (provider, model) = agent_manager.current_selection().await;
        let events = agent_manager.subscribe_events().await;

        Ok(Self {
            history: ConversationHistory::new(250),
            composer: ConversationComposer::new(placeholder, mode),
            streaming: StreamingResponse::new(mode),
            intent_router: IntentClassifier::new(),
            model_selector: ModelSelector::new(),
            current_mode: mode,
            agent_manager,
            is_active: false,
            stream_receiver: None,
            kernel_events: Some(events),
            activity_log: vec![ActivityEntry {
                timestamp: Utc::now(),
                message: format!(
                    "🚀 Ready in {} mode using {}::{}",
                    mode.display_name(),
                    provider,
                    model
                ),
            }],
            pending_tools: Vec::new(),
            active_provider: provider,
            active_model: model,
        })
    }

    /// Start a new conversation
    pub fn start_conversation(&mut self) {
        self.is_active = true;
        self.composer.set_focus(true);
        info!(
            mode = self.current_mode.display_name(),
            "conversation started"
        );
        self.history.add_system_message(
            format!("Started {} mode", self.current_mode.display_name()),
            self.current_mode,
        );
        self.log_activity(format!(
            "🧠 {} agent engaged",
            self.current_mode.display_name()
        ));
    }

    /// Handle user input and start streaming response
    pub async fn handle_input(&mut self, input: String) -> Result<()> {
        if input.trim().is_empty() {
            return Ok(());
        }

        debug!(
            mode = self.current_mode.display_name(),
            input_length = input.len(),
            "processing user input"
        );

        let context =
            ConversationContext::new(Some(self.current_mode), self.history.message_count());
        let intent_decision = self.intent_router.infer_intent(&context, &input);

        if intent_decision.mode != self.current_mode {
            info!(
                from = self.current_mode.display_name(),
                to = intent_decision.mode.display_name(),
                confidence = intent_decision.confidence,
                "auto-switching conversation mode"
            );
            self.history.add_system_message(
                format!(
                    "Switching to {} (confidence {:>3.0}%)\n{}",
                    intent_decision.mode.display_name(),
                    intent_decision.confidence * 100.0,
                    intent_decision.rationale
                ),
                intent_decision.mode,
            );
            self.agent_manager.switch_mode(intent_decision.mode).await?;
            self.current_mode = intent_decision.mode;
            self.log_activity(format!(
                "🤖 Auto-switched to {} mode",
                self.current_mode.display_name()
            ));
        }

        let runtime_config = self.agent_manager.config().await;
        if let Some(model_decision) = self.model_selector.select_model(&ModelRequest {
            intent: self.current_mode,
            config: &runtime_config,
        }) {
            let (current_provider, current_model) = self.agent_manager.current_selection().await;
            if current_provider != model_decision.provider_id
                || current_model != model_decision.model_id
            {
                self.history.add_system_message(
                    format!(
                        "Switching to {}::{}\n{}",
                        &model_decision.provider_id,
                        &model_decision.model_id,
                        model_decision.rationale
                    ),
                    self.current_mode,
                );
                self.agent_manager
                    .apply_model_decision(model_decision.provider_id, model_decision.model_id)
                    .await;
            }
        }

        // Add user message to history
        self.history
            .add_user_message(input.clone(), self.current_mode);

        // Start streaming response
        self.streaming.start_streaming();

        // Get streaming response from agent and store the receiver
        let stream_rx = self
            .agent_manager
            .continue_conversation(intent_decision, input)
            .await?;

        // Store the stream receiver for processing in the main loop
        self.stream_receiver = Some(stream_rx);

        Ok(())
    }

    /// Process streaming chunks (called from main loop)
    pub fn process_streaming_chunks(&mut self) {
        self.poll_kernel_events();
        if let Some(ref mut stream_rx) = self.stream_receiver {
            loop {
                match stream_rx.try_recv() {
                    Ok(event) => {
                        let stream_event: LlmStreamEvent = event.clone().into();
                        self.streaming.process_event(stream_event);

                        match event {
                            LlmEvent::TextDelta(_) | LlmEvent::ReasoningDelta(_) => {
                                self.history
                                    .set_streaming_message(self.streaming.get_response());
                            }
                            LlmEvent::ResponseComplete(_) => {
                                self.history
                                    .set_streaming_message(self.streaming.get_response());
                            }
                            LlmEvent::StreamComplete => {
                                self.finalize_streaming_session();
                                break;
                            }
                            LlmEvent::Error(error) => {
                                self.history.add_system_message(
                                    format!("Error: {}", error),
                                    self.current_mode,
                                );
                                self.clear_stream_state();
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                        break;
                    }
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                        self.finalize_streaming_session();
                        break;
                    }
                }
            }
        }
    }

    fn finalize_streaming_session(&mut self) {
        let final_response = self.streaming.get_response();

        if !final_response.is_empty() {
            info!(
                mode = self.current_mode.display_name(),
                response_length = final_response.len(),
                "received complete LLM response"
            );
            debug!(
                mode = self.current_mode.display_name(),
                response = %final_response,
                "LLM response content"
            );

            self.history
                .add_assistant_message(final_response.clone(), self.current_mode);
        }

        self.clear_stream_state();
    }

    fn clear_stream_state(&mut self) {
        self.history.clear_streaming_message();
        self.streaming.clear();
        self.stream_receiver = None;
    }

    fn poll_kernel_events(&mut self) {
        if let Some(mut rx) = self.kernel_events.take() {
            loop {
                let event = match rx.try_recv() {
                    Ok(event) => event,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Lagged(skipped)) => {
                        self.log_activity(format!(
                            "⚠️ Kernel event backlog dropped {skipped} messages"
                        ));
                        break;
                    }
                    Err(TryRecvError::Closed) => {
                        self.log_activity("⚠️ Kernel event stream closed".to_string());
                        return;
                    }
                };

                self.handle_kernel_event(event);
            }

            self.kernel_events = Some(rx);
        }
    }

    fn handle_kernel_event(&mut self, event: KernelEvent) {
        match event {
            KernelEvent::AgentNotification { notification, .. } => match notification {
                crate::core::kernel::AgentNotification::Message(message) => {
                    self.log_activity(message);
                }
                crate::core::kernel::AgentNotification::Structured { label, payload } => {
                    match label.as_str() {
                        "conversation:error" => {
                            if let Some(message) = payload.get("message").and_then(|m| m.as_str()) {
                                self.log_activity(format!("❗ Agent error: {message}"));
                                self.history.add_system_message(
                                    format!("Agent error: {message}"),
                                    self.current_mode,
                                );
                            }
                        }
                        "tool:review" => {
                            let requires = payload
                                .get("requires_approval")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            let tool = payload
                                .get("tool")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            if requires {
                                self.log_activity(format!(
                                    "🛑 Tool {tool} requires approval before execution"
                                ));
                                self.history.add_system_message(
                                    format!(
                                        "Tool {tool} is queued for approval. Use /approve to continue."
                                    ),
                                    self.current_mode,
                                );
                            } else {
                                self.log_activity(format!("🛠️ Tool {tool} approved automatically"));
                            }
                        }
                        "tool:result" => {
                            if let Some(summary) =
                                payload.get("summary").and_then(|value| value.as_str())
                            {
                                self.log_activity(format!("✅ {summary}"));
                                if let Some(output) =
                                    payload.get("output").and_then(|value| value.as_str())
                                {
                                    self.history.add_system_message(
                                        format!("Tool output:\n{}", output.trim()),
                                        self.current_mode,
                                    );
                                }
                            }
                        }
                        "tool:error" => {
                            if let Some(message) = payload.get("error").and_then(|v| v.as_str()) {
                                self.log_activity(format!("❌ Tool error: {message}"));
                                self.history.add_system_message(
                                    format!("Tool error: {message}"),
                                    self.current_mode,
                                );
                            }
                        }
                        "tool:invoke" => {
                            if let Some(tool) = payload.get("tool").and_then(|v| v.as_str()) {
                                self.log_activity(format!("🧰 Agent requested tool `{tool}`"));
                            }
                        }
                        "tool:pending" => {
                            if let Some(id_str) = payload.get("id").and_then(|v| v.as_str()) {
                                if let Ok(id) = Uuid::parse_str(id_str) {
                                    let tool = payload
                                        .get("tool")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("tool");
                                    let summary = format!(
                                        "🟡 Approval pending for {tool} (id {short})",
                                        short = &id_str[..8]
                                    );
                                    self.log_activity(summary.clone());
                                    if !self.pending_tools.iter().any(|item| item.id == id) {
                                        self.pending_tools.push(PendingToolItem { id, summary });
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            },
            KernelEvent::AgentLifecycle {
                agent_id,
                lifecycle,
            } => {
                let status = match lifecycle {
                    AgentLifecycle::Started => "started".to_string(),
                    AgentLifecycle::Restarted { attempt } => {
                        format!("restarted (attempt {attempt})")
                    }
                    AgentLifecycle::Panicked { attempt } => {
                        format!("panicked (attempt {attempt})")
                    }
                    AgentLifecycle::Exhausted => "exhausted".to_string(),
                    AgentLifecycle::Stopped => "stopped".to_string(),
                };
                self.log_activity(format!("ℹ️ {agent_id} {status}"));
            }
            KernelEvent::IntentState { state, .. } => {
                if let IntentState::Failed { error } = state {
                    self.log_activity(format!("❌ Intent failed: {error}"));
                }
            }
        }
    }

    fn log_activity(&mut self, message: impl Into<String>) {
        const MAX_ACTIVITY: usize = 64;
        self.activity_log.push(ActivityEntry {
            timestamp: Utc::now(),
            message: message.into(),
        });
        if self.activity_log.len() > MAX_ACTIVITY {
            let overflow = self.activity_log.len() - MAX_ACTIVITY;
            self.activity_log.drain(0..overflow);
        }
    }

    /// Switch to a different mode
    pub async fn switch_mode(&mut self, new_mode: BindrMode) -> Result<()> {
        if new_mode == self.current_mode {
            return Ok(());
        }

        let previous_mode = self.current_mode;
        info!(
            from = previous_mode.display_name(),
            to = new_mode.display_name(),
            "switching conversation mode"
        );

        // Switch agent mode
        self.agent_manager.switch_mode(new_mode).await?;

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
        self.log_activity(format!("🔁 Switched to {} mode", new_mode.display_name()));

        Ok(())
    }

    /// Handle key input
    pub async fn handle_key(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> Result<ConversationAction> {
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

    /// Refresh configuration for agent and client
    pub async fn update_config(&mut self, config: Config) {
        self.active_provider = config.selected_provider.clone();
        self.active_model = config.default_model.clone();
        self.agent_manager.update_config(config).await;
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
            SlashCommand::Home => Ok(ConversationAction::GoHome),
            SlashCommand::Bye => Ok(ConversationAction::Exit),
            SlashCommand::Help => {
                let help_text = get_help_text();
                self.history
                    .add_system_message(help_text, self.current_mode);
                Ok(ConversationAction::None)
            }
            SlashCommand::Model => {
                let invocation = ToolInvocation::new(
                    BindrTool::ListModels,
                    self.current_mode,
                    "List available models",
                );

                match self.agent_manager.review_tool_invocation(invocation).await {
                    Ok(outcome) => {
                        if outcome.requires_approval {
                            self.history.add_system_message(
                                "Model listing requires approval. Awaiting confirmation."
                                    .to_string(),
                                self.current_mode,
                            );
                            Ok(ConversationAction::None)
                        } else {
                            Ok(ConversationAction::ShowModelSelection)
                        }
                    }
                    Err(err) => {
                        self.history.add_system_message(
                            format!("Tool request rejected: {err}"),
                            self.current_mode,
                        );
                        Ok(ConversationAction::None)
                    }
                }
            }
            SlashCommand::Approve => {
                if self.pending_tools.is_empty() {
                    self.history.add_system_message(
                        "No pending tools to approve.".to_string(),
                        self.current_mode,
                    );
                    return Ok(ConversationAction::None);
                }

                let target_id = if let Some(arg) = command.argument() {
                    if let Ok(uuid) = Uuid::parse_str(arg.trim()) {
                        uuid
                    } else if let Ok(index) = arg.trim().parse::<usize>() {
                        let idx = index.saturating_sub(1);
                        if idx < self.pending_tools.len() {
                            self.pending_tools[idx].id
                        } else {
                            self.history.add_system_message(
                                format!("No pending tool at position {}", index),
                                self.current_mode,
                            );
                            return Ok(ConversationAction::None);
                        }
                    } else {
                        self.history.add_system_message(
                            "Provide a UUID or numeric index to approve a specific tool."
                                .to_string(),
                            self.current_mode,
                        );
                        return Ok(ConversationAction::None);
                    }
                } else {
                    // default to most recent pending tool
                    self.pending_tools.last().map(|item| item.id).unwrap()
                };

                if let Err(err) = self.agent_manager.approve_pending_tool(target_id).await {
                    self.history.add_system_message(
                        format!("Failed to approve tool: {err}"),
                        self.current_mode,
                    );
                    self.log_activity(format!("❌ Failed to approve tool: {err}"));
                } else {
                    if let Some(pos) = self
                        .pending_tools
                        .iter()
                        .position(|item| item.id == target_id)
                    {
                        let item = self.pending_tools.remove(pos);
                        self.history.add_system_message(
                            format!("Approved {}", item.summary),
                            self.current_mode,
                        );
                        self.log_activity(format!("✅ Approved {}", item.summary));
                    }
                }

                Ok(ConversationAction::None)
            }
        }
    }

    /// Get mode-specific placeholder text
    fn get_mode_placeholder(mode: BindrMode) -> String {
        match mode {
            BindrMode::Brainstorm => {
                "Share your ideas and let's explore possibilities...".to_string()
            }
            BindrMode::Plan => {
                "Describe your project and let's create a detailed plan...".to_string()
            }
            BindrMode::Execute => "What should I build? Describe the implementation...".to_string(),
            BindrMode::Document => {
                "What should I document? Describe the documentation needs...".to_string()
            }
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
                Constraint::Min(10),   // History
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
        let outer = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(8)])
            .split(area);

        self.render_status_bar(outer[0], buf);

        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(3, 4), Constraint::Ratio(1, 4)])
            .split(outer[1]);

        let convo = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(6), Constraint::Length(3)])
            .split(body[0]);

        self.history.clone().render(convo[0], buf);
        self.composer.clone().render(convo[1], buf);

        self.render_side_panel(body[1], buf);
    }

    fn render_status_bar(&self, area: Rect, buf: &mut Buffer) {
        let content = Line::from(vec![
            Span::styled(
                format!("Mode: {}", self.current_mode.display_name()),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  |  "),
            Span::styled(
                format!("Model: {}::{}", self.active_provider, self.active_model),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw("  |  "),
            Span::styled(
                format!("Messages: {}", self.history.message_count()),
                Style::default().fg(Color::Gray),
            ),
            Span::raw("  |  "),
            Span::styled(
                format!("Pending tools: {}", self.pending_tools.len()),
                Style::default().fg(Color::Magenta),
            ),
        ]);

        let paragraph = Paragraph::new(content).style(Style::default()).block(
            Block::default().borders(Borders::ALL).title(Span::styled(
                "Session",
                Style::default().add_modifier(Modifier::BOLD),
            )),
        );

        paragraph.render(area, buf);
    }

    fn render_side_panel(&self, area: Rect, buf: &mut Buffer) {
        let segments = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),
                Constraint::Percentage(60),
                Constraint::Min(4),
            ])
            .split(area);

        let pending_items: Vec<ListItem> = if self.pending_tools.is_empty() {
            vec![ListItem::new("No approvals pending")]
        } else {
            self.pending_tools
                .iter()
                .rev()
                .map(|item| ListItem::new(item.summary.clone()))
                .collect()
        };

        let pending = List::new(pending_items).block(Block::default().borders(Borders::ALL).title(
            Span::styled(
                "Pending Approvals",
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ));
        pending.render(segments[0], buf);

        let activity_items: Vec<ListItem> = self
            .activity_log
            .iter()
            .rev()
            .map(|entry| {
                let timestamp = entry.timestamp.format("%H:%M:%S").to_string();
                ListItem::new(format!("[{timestamp}] {}", entry.message))
            })
            .collect();

        let activity =
            List::new(activity_items).block(Block::default().borders(Borders::ALL).title(
                Span::styled("Activity", Style::default().add_modifier(Modifier::BOLD)),
            ));
        activity.render(segments[1], buf);

        let hints = Paragraph::new(vec![
            Line::from(vec![Span::styled(
                "Shortcuts",
                Style::default().add_modifier(Modifier::BOLD),
            )]),
            Line::from("Enter ▸ Send message"),
            Line::from("/mode ▸ Switch agent"),
            Line::from("/model ▸ Change provider/model"),
            Line::from("/approve [id?] ▸ Approve pending tool"),
        ])
        .block(Block::default().borders(Borders::ALL));

        hints.render(segments[2], buf);
    }
}
