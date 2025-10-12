use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::{Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{Mutex, broadcast, mpsc};
use tracing::warn;
use uuid::Uuid;

use crate::agents::{ModeAgentFactory, SharedRuntimeState, state::RuntimeState};
use crate::config::Config;
use crate::core::intent::IntentDecision;
use crate::core::kernel::{
    AgentId, AgentKernel, AgentNotification, IntentEnvelope, IntentPayload, KernelConfig,
    KernelEvent,
};
use crate::events::{BindrMode, ConversationRole};
use crate::llm::LlmEvent;
use crate::session::SessionManager;
use crate::tools::{
    ApplyPatchOptions, BindrTool, CommandOptions, DiffFileOptions, ListDirectoryOptions,
    ReadFileOptions, ToolDispatcher, ToolExecutionResult, ToolExecutor, ToolInvocation,
    ToolRequestOutcome, WriteFileOptions,
};

#[derive(Clone)]
struct PendingTool {
    id: Uuid,
    mode: BindrMode,
    invocation: ToolInvocation,
}

/// Runtime wiring that supervises per-mode agents through the micro-kernel.
pub struct AgentRuntime {
    kernel: Arc<Mutex<AgentKernel>>,
    state: SharedRuntimeState,
    agent_ids: HashMap<BindrMode, AgentId>,
    tool_executor: Arc<ToolExecutor>,
    pending_tools: Arc<Mutex<Vec<PendingTool>>>,
}

impl AgentRuntime {
    /// Wire the kernel, shared runtime state, and per-mode agents together.
    pub async fn new(config: Config, session_manager: SessionManager) -> Result<Self> {
        let state = Arc::new(Mutex::new(RuntimeState::new(
            config.clone(),
            session_manager,
        )));
        let kernel = Arc::new(Mutex::new(AgentKernel::new(KernelConfig::default())));
        let mut agent_ids = HashMap::new();
        let tool_executor = Arc::new(ToolExecutor::new(config.cwd.clone()));
        let pending_tools = Arc::new(Mutex::new(Vec::new()));

        {
            let mut guard = kernel.lock().await;
            for mode in [
                BindrMode::Brainstorm,
                BindrMode::Plan,
                BindrMode::Execute,
                BindrMode::Document,
            ] {
                let agent_id = format!("bindr.{}", mode.display_name().to_lowercase());
                let factory: Arc<dyn crate::core::kernel::AgentFactory> = Arc::new(
                    ModeAgentFactory::new(agent_id.clone(), mode, Arc::clone(&state)),
                );
                guard.register_agent(factory)?;
                agent_ids.insert(mode, agent_id);
            }
        }

        let mut event_rx = {
            let guard = kernel.lock().await;
            guard.subscribe()
        };
        let state_for_events = Arc::clone(&state);
        let pending_for_events = Arc::clone(&pending_tools);
        let kernel_for_events = Arc::clone(&kernel);
        let tool_executor_for_events = Arc::clone(&tool_executor);
        tokio::spawn(async move {
            loop {
                match event_rx.recv().await {
                    Ok(event) => {
                        if let Err(err) = process_kernel_event(
                            &state_for_events,
                            &pending_for_events,
                            &kernel_for_events,
                            &tool_executor_for_events,
                            event,
                        )
                        .await
                        {
                            warn!("failed to process kernel event: {err}");
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!("kernel event receiver lagged by {skipped} messages");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        if let Some(message) = detect_environment_warning(&config) {
            let guard = kernel.lock().await;
            guard.emit_notification("bindr.runtime", AgentNotification::Message(message));
        }

        Ok(Self {
            kernel,
            state,
            agent_ids,
            tool_executor,
            pending_tools,
        })
    }

    /// Dispatch a user intent to the appropriate mode agent.
    pub async fn dispatch_intent(
        &self,
        decision: IntentDecision,
        user_input: String,
    ) -> Result<mpsc::UnboundedReceiver<LlmEvent>> {
        let agent_id = self
            .agent_ids
            .get(&decision.mode)
            .cloned()
            .ok_or_else(|| anyhow!("no agent registered for mode {:?}", decision.mode))?;

        let (tx, rx) = mpsc::unbounded_channel();
        let payload = IntentPayload::new(user_input, Some(tx));
        let envelope = IntentEnvelope::new(decision, payload);
        {
            let mut state = self.state.lock().await;
            state.session_manager.switch_mode(envelope.decision.mode)?;
        }

        let guard = self.kernel.lock().await;
        guard
            .dispatch_intent(&agent_id, envelope)
            .await
            .map_err(|err| anyhow!(err))?;
        Ok(rx)
    }

    /// Switch the active session mode.
    pub async fn switch_mode(&self, mode: BindrMode) -> Result<()> {
        let mut state = self.state.lock().await;
        state.session_manager.switch_mode(mode)
    }

    /// Apply a model selection decision to runtime state.
    pub async fn apply_model_decision(&self, provider_id: String, model_id: String) {
        let mut state = self.state.lock().await;
        state.current_provider = provider_id;
        state.current_model = model_id;
    }

    /// Return the currently selected provider and model.
    pub async fn current_selection(&self) -> (String, String) {
        let state = self.state.lock().await;
        (state.current_provider.clone(), state.current_model.clone())
    }

    /// Snapshot the configuration for routing decisions.
    pub async fn config(&self) -> Config {
        let state = self.state.lock().await;
        state.config.clone()
    }

    /// Update runtime configuration, session state, and clients.
    pub async fn update_config(&self, config: Config) {
        let mut state = self.state.lock().await;
        state.config = config.clone();
        state.current_provider = config.selected_provider.clone();
        state.current_model = config.default_model.clone();
        state.session_manager.update_config(config.clone());
        if let Err(err) = state.session_manager.load_sessions() {
            warn!("failed to reload sessions after config update: {err}");
        }
        state.rebuild_llm();
    }

    /// Review a tool invocation for the currently active mode (used for slash commands/UI).
    pub async fn review_tool_invocation(
        &self,
        invocation: ToolInvocation,
    ) -> Result<ToolRequestOutcome> {
        let state = self.state.lock().await;
        let mode = state.session_manager.current_mode();
        let outcome = ToolDispatcher::review(mode, invocation)?;
        drop(state);

        self.publish_tool_review(mode, &outcome).await?;

        if !outcome.requires_approval {
            self.execute_and_publish(mode, outcome.invocation.clone())
                .await?;
        } else {
            self.enqueue_pending_tool(mode, outcome.invocation.clone(), None)
                .await?;
        }

        Ok(outcome)
    }

    /// Subscribe to kernel events for external consumers (UI, telemetry, etc.).
    pub async fn subscribe_events(&self) -> broadcast::Receiver<KernelEvent> {
        let guard = self.kernel.lock().await;
        guard.subscribe()
    }

    /// Approve a pending tool invocation, executing it immediately.
    pub async fn approve_pending_tool(&self, tool_id: Uuid) -> Result<()> {
        let pending = {
            let mut guard = self.pending_tools.lock().await;
            if let Some(index) = guard.iter().position(|item| item.id == tool_id) {
                guard.remove(index)
            } else {
                return Err(anyhow!("no pending tool with id {tool_id}"));
            }
        };

        self.execute_and_publish(pending.mode, pending.invocation)
            .await
    }

    async fn publish_tool_review(
        &self,
        mode: BindrMode,
        outcome: &ToolRequestOutcome,
    ) -> Result<()> {
        if let Some(agent_id) = self.agent_ids.get(&mode) {
            let guard = self.kernel.lock().await;
            guard.emit_notification(
                agent_id,
                AgentNotification::Structured {
                    label: "tool:review".to_string(),
                    payload: json!({
                        "mode": mode,
                        "tool": format!("{:?}", outcome.invocation.tool.kind()),
                        "requires_approval": outcome.requires_approval,
                    }),
                },
            );
        }
        Ok(())
    }

    async fn execute_tool(&self, invocation: ToolInvocation) -> Result<ToolExecutionResult> {
        let executor = Arc::clone(&self.tool_executor);
        executor.execute(&invocation).await
    }

    async fn execute_and_publish(&self, mode: BindrMode, invocation: ToolInvocation) -> Result<()> {
        match self.execute_tool(invocation.clone()).await {
            Ok(result) => {
                if let Some(agent_id) = self.agent_ids.get(&mode) {
                    let guard = self.kernel.lock().await;
                    guard.emit_notification(
                        agent_id,
                        AgentNotification::Structured {
                            label: "tool:result".to_string(),
                            payload: json!({
                                "mode": mode,
                                "summary": result.summary,
                                "output": result.output,
                            }),
                        },
                    );
                }
            }
            Err(err) => {
                if let Some(agent_id) = self.agent_ids.get(&mode) {
                    let guard = self.kernel.lock().await;
                    guard.emit_notification(
                        agent_id,
                        AgentNotification::Structured {
                            label: "tool:error".to_string(),
                            payload: json!({
                                "mode": mode,
                                "error": err.to_string(),
                            }),
                        },
                    );
                }
                return Err(err);
            }
        }
        Ok(())
    }

    async fn enqueue_pending_tool(
        &self,
        mode: BindrMode,
        invocation: ToolInvocation,
        note: Option<String>,
    ) -> Result<()> {
        let pending = PendingTool {
            id: Uuid::new_v4(),
            mode,
            invocation,
        };

        {
            let mut guard = self.pending_tools.lock().await;
            guard.push(pending.clone());
        }

        if let Some(agent_id) = self.agent_ids.get(&mode) {
            let guard = self.kernel.lock().await;
            guard.emit_notification(
                agent_id,
                AgentNotification::Structured {
                    label: "tool:pending".to_string(),
                    payload: json!({
                        "id": pending.id,
                        "mode": mode,
                        "tool": format!("{:?}", pending.invocation.tool.kind()),
                        "note": note,
                    }),
                },
            );
        }

        Ok(())
    }
}

#[derive(Deserialize)]
struct ToolInvokePayload {
    tool: String,
    #[serde(default)]
    mode: Option<BindrMode>,
    #[serde(default)]
    args: Value,
    #[serde(default)]
    note: Option<String>,
}

async fn process_kernel_event(
    state: &SharedRuntimeState,
    pending_tools: &Arc<Mutex<Vec<PendingTool>>>,
    kernel: &Arc<Mutex<AgentKernel>>,
    tool_executor: &Arc<ToolExecutor>,
    event: KernelEvent,
) -> Result<()> {
    if let KernelEvent::AgentNotification {
        notification: AgentNotification::Structured { label, payload },
        ..
    } = event
    {
        match label.as_str() {
            "tool:invoke" => {
                handle_tool_invoke(state, pending_tools, kernel, tool_executor, payload).await?;
            }
            "conversation:user_message" => {
                persist_conversation_entry(state, &payload, ConversationRole::User).await?;
            }
            "conversation:assistant_message" => {
                persist_conversation_entry(state, &payload, ConversationRole::Assistant).await?;
                let mut guard = state.lock().await;
                if let Err(err) = guard.session_manager.save_current_session() {
                    warn!("failed to save session after assistant response: {err}");
                }
            }
            "tool:result" => {
                if let Some(summary) = payload.get("summary").and_then(|v| v.as_str()) {
                    let mode = payload
                        .get("mode")
                        .and_then(|v| serde_json::from_value(v.clone()).ok())
                        .unwrap_or(BindrMode::Execute);
                    log_system_message(state, mode, summary).await?;
                }
            }
            "tool:error" => {
                if let Some(message) = payload.get("error").and_then(|v| v.as_str()) {
                    let mode = payload
                        .get("mode")
                        .and_then(|v| serde_json::from_value(v.clone()).ok())
                        .unwrap_or(BindrMode::Execute);
                    let error_message = format!("Tool error: {message}");
                    log_system_message(state, mode, error_message).await?;
                }
            }
            _ => {}
        }
    }

    Ok(())
}

async fn handle_tool_invoke(
    state: &SharedRuntimeState,
    pending_tools: &Arc<Mutex<Vec<PendingTool>>>,
    kernel: &Arc<Mutex<AgentKernel>>,
    tool_executor: &Arc<ToolExecutor>,
    payload: Value,
) -> Result<()> {
    let call: ToolInvokePayload = serde_json::from_value(payload)
        .map_err(|err| anyhow!("invalid tool invoke payload: {err}"))?;

    let mode = call
        .mode
        .ok_or_else(|| anyhow!("tool invoke payload missing mode"))?;
    let invocation = build_tool_invocation(mode, &call.tool, &call.args)?;

    let tool_name = call.tool.as_str();
    let invoke_message = format!("Invoking tool `{tool_name}`");
    log_system_message(state, mode, invoke_message).await?;

    let outcome = ToolDispatcher::review(mode, invocation.clone())?;

    {
        let agent_id = format!("bindr.{}", mode.display_name().to_lowercase());
        let guard = kernel.lock().await;
        guard.emit_notification(
            &agent_id,
            AgentNotification::Structured {
                label: "tool:review".to_string(),
                payload: json!({
                    "mode": mode,
                    "tool": format!("{:?}", outcome.invocation.tool.kind()),
                    "requires_approval": outcome.requires_approval,
                }),
            },
        );
    }

    if outcome.requires_approval {
        enqueue_pending_tool_internal(
            pending_tools,
            kernel,
            mode,
            outcome.invocation.clone(),
            call.note.clone(),
        )
        .await?;
    } else {
        execute_tool_internal(tool_executor, kernel, mode, outcome.invocation.clone()).await?;
    }

    Ok(())
}

async fn enqueue_pending_tool_internal(
    pending_tools: &Arc<Mutex<Vec<PendingTool>>>,
    kernel: &Arc<Mutex<AgentKernel>>,
    mode: BindrMode,
    invocation: ToolInvocation,
    note: Option<String>,
) -> Result<()> {
    let pending = PendingTool {
        id: Uuid::new_v4(),
        mode,
        invocation,
    };

    {
        let mut guard = pending_tools.lock().await;
        guard.push(pending.clone());
    }

    let agent_id = format!("bindr.{}", mode.display_name().to_lowercase());
    let guard = kernel.lock().await;
    guard.emit_notification(
        &agent_id,
        AgentNotification::Structured {
            label: "tool:pending".to_string(),
            payload: json!({
                "id": pending.id,
                "mode": mode,
                "tool": format!("{:?}", pending.invocation.tool.kind()),
                "note": note,
            }),
        },
    );

    Ok(())
}

async fn execute_tool_internal(
    tool_executor: &Arc<ToolExecutor>,
    kernel: &Arc<Mutex<AgentKernel>>,
    mode: BindrMode,
    invocation: ToolInvocation,
) -> Result<()> {
    let result = tool_executor.execute(&invocation).await;
    let agent_id = format!("bindr.{}", mode.display_name().to_lowercase());
    let guard = kernel.lock().await;
    match result {
        Ok(output) => {
            guard.emit_notification(
                &agent_id,
                AgentNotification::Structured {
                    label: "tool:result".to_string(),
                    payload: json!({
                        "mode": mode,
                        "summary": output.summary,
                        "output": output.output,
                    }),
                },
            );
        }
        Err(err) => {
            guard.emit_notification(
                &agent_id,
                AgentNotification::Structured {
                    label: "tool:error".to_string(),
                    payload: json!({
                        "mode": mode,
                        "error": err.to_string(),
                    }),
                },
            );
            return Err(err);
        }
    }
    Ok(())
}

async fn persist_conversation_entry(
    state: &SharedRuntimeState,
    payload: &Value,
    role: ConversationRole,
) -> Result<()> {
    let content = payload
        .get("content")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();

    if content.is_empty() {
        return Ok(());
    }

    let mode_value = payload
        .get("mode")
        .cloned()
        .ok_or_else(|| anyhow!("kernel payload missing mode field"))?;
    let mode: BindrMode = serde_json::from_value(mode_value)
        .map_err(|err| anyhow!("failed to parse mode from kernel payload: {err}"))?;

    let mut guard = state.lock().await;
    guard
        .session_manager
        .add_conversation_entry(role, content, mode)?;
    Ok(())
}

async fn log_system_message(
    state: &SharedRuntimeState,
    mode: BindrMode,
    message: impl Into<String>,
) -> Result<()> {
    let mut guard = state.lock().await;
    guard
        .session_manager
        .add_conversation_entry(ConversationRole::System, message.into(), mode)?;
    Ok(())
}

fn detect_environment_warning(config: &Config) -> Option<String> {
    let home = dirs::home_dir()?;
    if config.cwd == home {
        Some(
            "⚠️ Bindr detected your home directory as the active workspace. For best results, run Bindr inside a project directory.".to_string(),
        )
    } else {
        None
    }
}

fn build_tool_invocation(mode: BindrMode, tool: &str, args: &Value) -> Result<ToolInvocation> {
    let invocation = match tool {
        "read_file" => {
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("read_file requires a path"))?;
            let max_bytes = args
                .get("max_bytes")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);
            ToolInvocation::new(
                BindrTool::ReadFile(ReadFileOptions {
                    path: PathBuf::from(path),
                    max_bytes,
                }),
                mode,
                format!("Read file {path}"),
            )
        }
        "list_directory" => {
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("list_directory requires a path"))?;
            let recursive = args
                .get("recursive")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let include_hidden = args
                .get("include_hidden")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let max_entries = args
                .get("max_entries")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);
            ToolInvocation::new(
                BindrTool::ListDirectory(ListDirectoryOptions {
                    path: PathBuf::from(path),
                    recursive,
                    include_hidden,
                    max_entries,
                }),
                mode,
                format!("List directory {path}"),
            )
        }
        "write_file" => {
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("write_file requires a path"))?;
            let contents = args
                .get("contents")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("write_file requires contents"))?;
            let create_if_missing = args
                .get("create_if_missing")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            ToolInvocation::new(
                BindrTool::WriteFile(WriteFileOptions {
                    path: PathBuf::from(path),
                    contents: contents.to_string(),
                    create_if_missing,
                }),
                mode,
                format!("Write file {path}"),
            )
        }
        "diff_file" => {
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("diff_file requires a path"))?;
            let context_lines = args
                .get("context_lines")
                .and_then(|v| v.as_u64())
                .unwrap_or(3) as usize;
            ToolInvocation::new(
                BindrTool::DiffFile(DiffFileOptions {
                    path: PathBuf::from(path),
                    context_lines,
                }),
                mode,
                format!("Diff file {path}"),
            )
        }
        "apply_patch" => {
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("apply_patch requires a path"))?;
            let patch = args
                .get("patch")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("apply_patch requires patch text"))?;
            ToolInvocation::new(
                BindrTool::ApplyPatch(ApplyPatchOptions {
                    path: PathBuf::from(path),
                    patch: patch.to_string(),
                }),
                mode,
                format!("Apply patch to {path}"),
            )
        }
        "run_command" => {
            let command = args
                .get("command")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("run_command requires a command"))?;
            let arg_list = args
                .get("args")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|value| value.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let working_dir = args
                .get("working_dir")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let allow_network = args
                .get("allow_network")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            ToolInvocation::new(
                BindrTool::RunCommand(CommandOptions {
                    command: command.to_string(),
                    args: arg_list,
                    working_dir: PathBuf::from(working_dir),
                    allow_network,
                }),
                mode,
                format!("Run command `{command}`"),
            )
        }
        "web_search" => {
            let query = args
                .get("query")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("web_search requires a query"))?;
            let max_results = args
                .get("max_results")
                .and_then(|v| v.as_u64())
                .map(|v| v as u8);
            ToolInvocation::new(
                BindrTool::WebSearch(crate::tools::WebSearchOptions {
                    query: query.to_string(),
                    max_results,
                }),
                mode,
                format!("Search the web for \"{query}\""),
            )
        }
        other => return Err(anyhow!("unknown tool `{other}`")),
    };

    Ok(invocation)
}
