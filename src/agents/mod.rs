pub mod state;

use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tracing::debug;

use crate::core::kernel::{
    AgentBroker, AgentFactory, AgentId, AgentNotification, IntentEnvelope, KernelAgent,
};
use crate::events::{BindrMode, ConversationEntry, ConversationRole};
use crate::llm::{LlmEvent, LlmMessage, LlmRequest};
use crate::prompts;

use self::state::RuntimeState;

pub type SharedRuntimeState = Arc<Mutex<RuntimeState>>;

pub struct ModeAgentFactory {
    agent_id: AgentId,
    mode: BindrMode,
    state: SharedRuntimeState,
}

impl ModeAgentFactory {
    pub fn new(agent_id: AgentId, mode: BindrMode, state: SharedRuntimeState) -> Self {
        Self {
            agent_id,
            mode,
            state,
        }
    }
}

#[async_trait]
impl AgentFactory for ModeAgentFactory {
    fn agent_id(&self) -> &str {
        &self.agent_id
    }

    fn spawn(&self) -> Box<dyn KernelAgent> {
        Box::new(ModeAgent {
            mode: self.mode,
            state: Arc::clone(&self.state),
        })
    }
}

struct ModeAgent {
    mode: BindrMode,
    state: SharedRuntimeState,
}

#[async_trait]
impl KernelAgent for ModeAgent {
    async fn handle_intent(&mut self, intent: IntentEnvelope, broker: &AgentBroker) -> Result<()> {
        if intent.decision.mode != self.mode {
            broker.notify(AgentNotification::Message(format!(
                "intent routed to mismatched agent: expected {:?}, received {:?}",
                self.mode, intent.decision.mode
            )));
            return Ok(());
        }

        let user_message = intent.payload.user_input.clone();
        let mut assistant_response = String::new();
        let mut response_tx = intent.payload.response_tx.clone();
        let mut llm_stream = {
            let guard = self.state.lock().await;
            let history_snapshot = guard.session_manager.conversation_history();
            let provider_id = if guard.current_provider.is_empty() {
                guard.config.selected_provider.clone()
            } else {
                guard.current_provider.clone()
            };
            let model_id = if guard.current_model.is_empty() {
                guard.config.default_model.clone()
            } else {
                guard.current_model.clone()
            };
            let system_prompt =
                build_system_prompt(&history_snapshot, self.mode, &provider_id, &model_id);
            let messages = build_messages(&history_snapshot, &system_prompt, user_message.clone());

            let request = LlmRequest::new(messages, self.mode)
                .with_temperature(0.4)
                .with_max_tokens(2000)
                .with_provider(provider_id.clone())
                .with_model(model_id.clone());

            let client = guard.llm_client.clone();
            drop(guard);

            client
                .stream_response(request)
                .await
                .context("failed to start LLM stream")?
        };

        broker.notify(AgentNotification::Structured {
            label: "conversation:user_message".to_string(),
            payload: json!({
                "intent_id": intent.id,
                "mode": self.mode,
                "content": user_message,
            }),
        });

        while let Some(event) = llm_stream.recv().await {
            match &event {
                LlmEvent::TextDelta(delta) | LlmEvent::ReasoningDelta(delta) => {
                    assistant_response.push_str(delta);
                }
                LlmEvent::ResponseComplete(text) => {
                    assistant_response.push_str(text);
                }
                LlmEvent::Error(error) => {
                    broker.notify(AgentNotification::Structured {
                        label: "conversation:error".to_string(),
                        payload: json!({
                            "intent_id": intent.id,
                            "message": error,
                        }),
                    });
                }
                LlmEvent::StreamComplete => {
                    // no-op
                }
            }

            if let Some(tx) = response_tx.as_ref()
                && tx.send(event.clone()).is_err()
            {
                debug!(agent = %broker.agent_id(), "response channel closed by consumer");
                response_tx = None;
            }
        }

        let tool_calls = extract_tool_calls(&assistant_response);
        if !tool_calls.is_empty() {
            for call in &tool_calls {
                let mut payload = call.as_object().cloned().unwrap_or_default();
                payload.insert(
                    "mode".to_string(),
                    serde_json::to_value(self.mode).unwrap_or(Value::Null),
                );
                broker.notify(AgentNotification::Structured {
                    label: "tool:invoke".to_string(),
                    payload: Value::Object(payload.clone()),
                });
            }

            let summaries: Vec<String> = tool_calls
                .iter()
                .map(|call| {
                    call.get("tool")
                        .and_then(|t| t.as_str())
                        .map(|name| format!("Invoking tool `{name}`"))
                        .unwrap_or_else(|| "Invoking tool".to_string())
                })
                .collect();

            broker.notify(AgentNotification::Structured {
                label: "conversation:assistant_message".to_string(),
                payload: json!({
                    "intent_id": intent.id,
                    "mode": self.mode,
                    "content": summaries.join("\n"),
                }),
            });
        } else {
            broker.notify(AgentNotification::Structured {
                label: "conversation:assistant_message".to_string(),
                payload: json!({
                    "intent_id": intent.id,
                    "mode": self.mode,
                    "content": assistant_response,
                }),
            });
        }

        Ok(())
    }
}

fn build_messages(
    history: &[ConversationEntry],
    system_prompt: &str,
    user_message: String,
) -> Vec<LlmMessage> {
    let mut messages = Vec::with_capacity(history.len() + 2);
    messages.push(LlmMessage {
        role: "system".to_string(),
        content: system_prompt.to_string(),
    });

    for entry in history.iter() {
        messages.push(LlmMessage {
            role: entry.role.to_string(),
            content: entry.content.clone(),
        });
    }

    messages.push(LlmMessage {
        role: "user".to_string(),
        content: user_message,
    });

    messages
}

fn build_system_prompt(
    history: &[ConversationEntry],
    mode: BindrMode,
    provider: &str,
    model: &str,
) -> String {
    let mut prompt = prompts::mode_prompt(mode, provider, model);
    if let Some(context) = build_context_summary(history) {
        prompt.push_str("\n\nContext from previous work:\n");
        prompt.push_str(&context);
    }
    prompt
}

fn extract_tool_calls(text: &str) -> Vec<Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Object(obj)) if obj.get("tool").is_some() => vec![Value::Object(obj)],
        Ok(Value::Array(items)) => items
            .into_iter()
            .filter(|item| item.get("tool").is_some())
            .collect(),
        _ => Vec::new(),
    }
}

fn build_context_summary(history: &[ConversationEntry]) -> Option<String> {
    let brainstorm = summarize_mode(history, BindrMode::Brainstorm);
    let plan = summarize_mode(history, BindrMode::Plan);
    let execute = summarize_mode(history, BindrMode::Execute);

    let mut parts = Vec::new();
    if let Some(text) = brainstorm {
        parts.push(format!("Brainstorming: {text}"));
    }
    if let Some(text) = plan {
        parts.push(format!("Planning: {text}"));
    }
    if let Some(text) = execute {
        parts.push(format!("Execution: {text}"));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn summarize_mode(history: &[ConversationEntry], mode: BindrMode) -> Option<String> {
    let mut entries: Vec<&ConversationEntry> = history
        .iter()
        .filter(|entry| entry.mode == mode && matches!(entry.role, ConversationRole::Assistant))
        .collect();

    if entries.is_empty() {
        return None;
    }

    // Keep only the most recent 5 assistant messages for brevity.
    if entries.len() > 5 {
        entries.drain(0..(entries.len() - 5));
    }

    let mut summary = entries
        .iter()
        .map(|entry| entry.content.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    if summary.len() > 500 {
        summary.truncate(500);
        summary.push_str("...");
    }

    Some(summary)
}
