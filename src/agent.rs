use crate::config::Config;
use crate::events::{BindrMode, ConversationRole, ConversationEntry, ProjectState};
use crate::llm::{LlmClient, LlmRequest, LlmMessage, LlmEvent};
use crate::session::SessionManager;
use anyhow::Result;
use tokio::sync::mpsc;

/// Agent orchestrator that manages different modes and their interactions
#[derive(Clone)]
pub struct AgentOrchestrator {
    #[allow(dead_code)]
    config: Config,
    llm_client: LlmClient,
    #[allow(dead_code)]
    session_manager: SessionManager,
    current_mode: BindrMode,
    conversation_history: Vec<ConversationEntry>,
}

impl AgentOrchestrator {
    pub fn new(config: Config, session_manager: SessionManager) -> Self {
        let llm_client = LlmClient::new(config.clone());
        
        Self {
            config,
            llm_client,
            session_manager,
            current_mode: BindrMode::Brainstorm,
            conversation_history: Vec::new(),
        }
    }

    /// Start a new conversation in the current mode
    #[allow(dead_code)]
    pub async fn start_conversation(
        &mut self,
        initial_prompt: Option<String>,
    ) -> Result<mpsc::Receiver<LlmEvent>> {
        let mut messages = vec![LlmMessage {
            role: "system".to_string(),
            content: self.get_system_prompt(),
        }];

        if let Some(prompt) = initial_prompt {
            messages.push(LlmMessage {
                role: "user".to_string(),
                content: prompt,
            });
        }

        let request = LlmRequest::new(messages, self.current_mode)
            .with_max_tokens(16000);
        self.llm_client.stream_response(request).await
    }

    /// Continue the conversation with a new user message
    pub async fn continue_conversation(
        &mut self,
        user_message: String,
    ) -> Result<mpsc::UnboundedReceiver<String>> {
        // Add user message to history
        self.add_to_history(ConversationRole::User, user_message.clone());

        // Build conversation context
        let mut messages = vec![LlmMessage {
            role: "system".to_string(),
            content: self.get_system_prompt_for_mode(self.current_mode),
        }];

        // Add conversation history
        for entry in &self.conversation_history {
            messages.push(LlmMessage {
                role: entry.role.to_string(),
                content: entry.content.clone(),
            });
        }

        // Add current user message
        messages.push(LlmMessage {
            role: "user".to_string(),
            content: user_message,
        });

        let request = LlmRequest::new(messages, self.current_mode)
            .with_max_tokens(4000);
        let mut llm_rx = self.llm_client.stream_response(request).await?;
        
        // Convert LLM events to simple string chunks
        let (tx, rx) = mpsc::unbounded_channel();
        
        tokio::spawn(async move {
            while let Some(event) = llm_rx.recv().await {
                match event {
                    LlmEvent::TextDelta(chunk) => {
                        let _ = tx.send(chunk);
                    }
                    LlmEvent::ResponseComplete(content) => {
                        let _ = tx.send(content);
                    }
                    LlmEvent::ReasoningDelta(_reasoning) => {
                        // Optionally forward reasoning content; currently ignored to avoid UX clutter
                    }
                    LlmEvent::StreamComplete => {
                        break;
                    }
                    LlmEvent::Error(error) => {
                        let _ = tx.send(format!("Error: {}", error));
                        break;
                    }
                }
            }
        });
        
        Ok(rx)
    }

    /// Switch to a different mode
    pub async fn switch_mode(&mut self, new_mode: BindrMode) -> Result<()> {
        if new_mode == self.current_mode {
            return Ok(());
        }

        // Save current conversation state
        self.save_conversation_state().await?;

        // Switch mode
        self.current_mode = new_mode;

        // Load conversation state for new mode
        self.load_conversation_state().await?;

        Ok(())
    }

    /// Get the current mode
    #[allow(dead_code)]
    pub fn current_mode(&self) -> BindrMode {
        self.current_mode
    }

    /// Get conversation history
    #[allow(dead_code)]
    pub fn conversation_history(&self) -> &[ConversationEntry] {
        &self.conversation_history
    }

    /// Add an entry to conversation history
    pub fn add_to_history(&mut self, role: ConversationRole, content: String) {
        self.conversation_history.push(ConversationEntry {
            mode: self.current_mode,
            role,
            content,
            timestamp: chrono::Utc::now(),
        });
    }

    /// Get system prompt for current mode
    fn get_system_prompt(&self) -> String {
        let base_prompt = match self.current_mode {
            BindrMode::Brainstorm => {
                "You are a creative brainstorming assistant for Bindr. Help users explore ideas, think outside the box, and generate innovative concepts. Be concise, creative, and encouraging. Ask probing questions to help users refine their ideas. When the user seems ready to move forward with a concept, suggest creating a project and moving to planning mode."
            }
            BindrMode::Plan => {
                "You are a detailed project planning assistant for Bindr. Create comprehensive, structured plans with clear steps, architecture decisions, and implementation roadmaps. Be thorough, organized, and practical. Focus on actionable items and realistic timelines. Generate a detailed plan that can be used for implementation. When the plan is complete, suggest moving to execution mode."
            }
            BindrMode::Execute => {
                "You are a code implementation assistant for Bindr. Generate clean, well-documented code based on project plans. Follow best practices, include error handling, and write tests when appropriate. Be precise and efficient in your implementations. Focus on creating working, production-ready code. When implementation is complete, suggest moving to documentation mode."
            }
            BindrMode::Document => {
                "You are a documentation specialist for Bindr. Create clear, comprehensive documentation that explains what was built, how it works, and how to use it. Be thorough but concise, and focus on practical information for developers and users. Generate documentation that makes the project accessible and maintainable."
            }
        };

        // Add context from previous modes if available
        let context = self.get_mode_context();
        if !context.is_empty() {
            format!("{}\n\nContext from previous work:\n{}", base_prompt, context)
        } else {
            base_prompt.to_string()
        }
    }

    /// Get context from previous modes
    fn get_mode_context(&self) -> String {
        let mut context_parts = Vec::new();

        // Add brainstorm context
        if let Some(brainstorm_context) = self.get_brainstorm_context() {
            context_parts.push(format!("Brainstorming: {}", brainstorm_context));
        }

        // Add plan context
        if let Some(plan_context) = self.get_plan_context() {
            context_parts.push(format!("Planning: {}", plan_context));
        }

        // Add execution context
        if let Some(exec_context) = self.get_execution_context() {
            context_parts.push(format!("Implementation: {}", exec_context));
        }

        context_parts.join("\n")
    }

    /// Get brainstorm context summary
    fn get_brainstorm_context(&self) -> Option<String> {
        // Look for brainstorm entries in conversation history
        let brainstorm_entries: Vec<_> = self.conversation_history
            .iter()
            .filter(|entry| matches!(entry.role, ConversationRole::Assistant))
            .collect();

        if brainstorm_entries.is_empty() {
            return None;
        }

        // Create a summary of key brainstorm points
        let summary = brainstorm_entries
            .iter()
            .map(|entry| entry.content.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        // Truncate if too long
        if summary.len() > 500 {
            Some(format!("{}...", &summary[..500]))
        } else {
            Some(summary)
        }
    }

    /// Get plan context summary
    fn get_plan_context(&self) -> Option<String> {
        // This would typically come from a saved plan file
        // For now, return None as we haven't implemented plan persistence yet
        None
    }

    /// Get execution context summary
    fn get_execution_context(&self) -> Option<String> {
        // This would typically come from execution logs
        // For now, return None as we haven't implemented execution tracking yet
        None
    }

    /// Save conversation state for current mode
    async fn save_conversation_state(&mut self) -> Result<()> {
        // This would save the conversation to the appropriate mode file
        // For now, we'll just keep it in memory
        Ok(())
    }

    /// Load conversation state for current mode
    async fn load_conversation_state(&mut self) -> Result<()> {
        // This would load the conversation from the appropriate mode file
        // For now, we'll just keep it in memory
        Ok(())
    }

    /// Process a complete response and add it to history
    pub fn process_complete_response(&mut self, response: String) {
        self.add_to_history(ConversationRole::Assistant, response);
    }

    /// Get project state summary
    #[allow(dead_code)]
    pub fn get_project_state(&self) -> ProjectState {
        ProjectState {
            name: "current".to_string(),
            path: std::path::PathBuf::new(),
            current_mode: self.current_mode,
            created_at: chrono::Utc::now().to_rfc3339(),
            last_modified: chrono::Utc::now().to_rfc3339(),
            bindr_md_content: String::new(),
            conversation_history: self.conversation_history.clone(),
            conversation_count: self.conversation_history.len(),
            last_activity: chrono::Utc::now(),
        }
    }

    /// Check if we should suggest mode transition
    #[allow(dead_code)]
    pub fn should_suggest_transition(&self) -> Option<BindrMode> {
        match self.current_mode {
            BindrMode::Brainstorm => {
                // Suggest moving to Plan if we have a good concept
                if self.conversation_history.len() >= 3 {
                    Some(BindrMode::Plan)
                } else {
                    None
                }
            }
            BindrMode::Plan => {
                // Suggest moving to Execute if we have a complete plan
                if self.conversation_history.len() >= 2 {
                    Some(BindrMode::Execute)
                } else {
                    None
                }
            }
            BindrMode::Execute => {
                // Suggest moving to Document if we have implementation
                if self.conversation_history.len() >= 1 {
                    Some(BindrMode::Document)
                } else {
                    None
                }
            }
            BindrMode::Document => {
                // Document mode is terminal, no automatic transition
                None
            }
        }
    }

    /// Get system prompt for a specific mode
    fn get_system_prompt_for_mode(&self, mode: BindrMode) -> String {
        match mode {
            BindrMode::Brainstorm => {
                "You are a creative brainstorming assistant. Help users explore ideas, think outside the box, and generate innovative concepts. Be enthusiastic, creative, and encouraging. Ask probing questions to help users refine their ideas.".to_string()
            }
            BindrMode::Plan => {
                "You are a detailed project planning assistant. Create comprehensive, structured plans with clear steps, architecture decisions, and implementation roadmaps. Be thorough, organized, and practical. Focus on actionable items and realistic timelines.".to_string()
            }
            BindrMode::Execute => {
                "You are a code implementation assistant. Generate clean, well-documented code based on project plans. Follow best practices, include error handling, and write tests when appropriate. Be precise and efficient in your implementations.".to_string()
            }
            BindrMode::Document => {
                "You are a documentation specialist. Create clear, comprehensive documentation that explains what was built, how it works, and how to use it. Be thorough but concise, and focus on practical information for developers and users.".to_string()
            }
        }
    }

    /// Get transition suggestion message
    #[allow(dead_code)]
    pub fn get_transition_suggestion(&self) -> Option<String> {
        if let Some(next_mode) = self.should_suggest_transition() {
            match next_mode {
                BindrMode::Plan => Some("Ready to create a detailed plan? Press 'P' to switch to Planning mode.".to_string()),
                BindrMode::Execute => Some("Ready to start implementation? Press 'E' to switch to Execution mode.".to_string()),
                BindrMode::Document => Some("Ready to document your work? Press 'D' to switch to Documentation mode.".to_string()),
                _ => None,
            }
        } else {
            None
        }
    }
}

/// Agent manager that coordinates multiple agents
#[derive(Clone)]
pub struct AgentManager {
    orchestrator: AgentOrchestrator,
    #[allow(dead_code)]
    is_active: bool,
}

impl AgentManager {
    pub fn new(config: Config, session_manager: SessionManager) -> Self {
        Self {
            orchestrator: AgentOrchestrator::new(config, session_manager),
            is_active: false,
        }
    }

    /// Start the agent manager
    #[allow(dead_code)]
    pub fn start(&mut self) {
        self.is_active = true;
    }

    /// Stop the agent manager
    #[allow(dead_code)]
    pub fn stop(&mut self) {
        self.is_active = false;
    }

    /// Check if the agent manager is active
    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// Get a mutable reference to the orchestrator
    pub fn orchestrator_mut(&mut self) -> &mut AgentOrchestrator {
        &mut self.orchestrator
    }

    /// Get a reference to the orchestrator
    #[allow(dead_code)]
    pub fn orchestrator(&self) -> &AgentOrchestrator {
        &self.orchestrator
    }
}
