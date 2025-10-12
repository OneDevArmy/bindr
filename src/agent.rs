use std::sync::Arc;

use crate::config::Config;
use crate::core::agent_runtime::AgentRuntime;
use crate::core::intent::IntentDecision;
use crate::core::kernel::KernelEvent;
use crate::events::BindrMode;
use crate::llm::LlmEvent;
use crate::session::SessionManager;
use crate::tools::{ToolInvocation, ToolRequestOutcome};
use anyhow::Result;
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

/// Facade over the agent runtime that exposes high-level orchestration operations to the UI.
#[derive(Clone)]
pub struct AgentManager {
    runtime: Arc<AgentRuntime>,
}

impl AgentManager {
    pub async fn new(config: Config, session_manager: SessionManager) -> Result<Self> {
        let runtime = AgentRuntime::new(config, session_manager).await?;
        Ok(Self {
            runtime: Arc::new(runtime),
        })
    }

    pub async fn config(&self) -> Config {
        self.runtime.config().await
    }

    pub async fn current_selection(&self) -> (String, String) {
        self.runtime.current_selection().await
    }

    pub async fn apply_model_decision(&self, provider_id: String, model_id: String) {
        self.runtime
            .apply_model_decision(provider_id, model_id)
            .await;
    }

    pub async fn switch_mode(&self, new_mode: BindrMode) -> Result<()> {
        self.runtime.switch_mode(new_mode).await
    }

    pub async fn continue_conversation(
        &self,
        decision: IntentDecision,
        user_input: String,
    ) -> Result<mpsc::UnboundedReceiver<LlmEvent>> {
        self.runtime.dispatch_intent(decision, user_input).await
    }

    pub async fn update_config(&self, config: Config) {
        self.runtime.update_config(config).await;
    }

    pub async fn review_tool_invocation(
        &self,
        invocation: ToolInvocation,
    ) -> Result<ToolRequestOutcome> {
        self.runtime.review_tool_invocation(invocation).await
    }

    pub async fn subscribe_events(&self) -> broadcast::Receiver<KernelEvent> {
        self.runtime.subscribe_events().await
    }

    pub async fn approve_pending_tool(&self, tool_id: Uuid) -> Result<()> {
        self.runtime.approve_pending_tool(tool_id).await
    }
}
