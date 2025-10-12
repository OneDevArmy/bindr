#![allow(dead_code)]

use std::{collections::HashMap, panic::AssertUnwindSafe, sync::Arc, time::Duration};

use anyhow::Result;
use async_trait::async_trait;
use futures::FutureExt;
use serde_json::Value;
use thiserror::Error;
use tokio::{
    sync::{broadcast, mpsc},
    time::sleep,
};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::core::intent::IntentDecision;
use crate::llm::LlmEvent;

/// Stable identifier assigned to each agent instance managed by the kernel.
pub type AgentId = String;

#[derive(Debug, Clone)]
/// Message envelope the kernel hands to an agent for execution.
pub struct IntentEnvelope {
    pub id: Uuid,
    pub decision: IntentDecision,
    pub payload: IntentPayload,
}

impl IntentEnvelope {
    /// Build an intent envelope with a freshly generated correlation id.
    pub fn new(decision: IntentDecision, payload: IntentPayload) -> Self {
        Self {
            id: Uuid::new_v4(),
            decision,
            payload,
        }
    }
}

#[derive(Debug, Clone)]
/// Additional data supplied alongside an intent dispatch.
pub struct IntentPayload {
    pub user_input: String,
    pub response_tx: Option<mpsc::UnboundedSender<LlmEvent>>,
}

impl IntentPayload {
    /// Create a payload for the provided user input and optional streaming channel.
    pub fn new(user_input: String, response_tx: Option<mpsc::UnboundedSender<LlmEvent>>) -> Self {
        Self {
            user_input,
            response_tx,
        }
    }
}

#[derive(Debug, Clone)]
/// Commands transmitted from the kernel to an agent mailbox.
pub enum KernelCommand {
    Dispatch(IntentEnvelope),
    Shutdown,
}

#[derive(Debug, Clone)]
/// Result states emitted for intents as the agent progresses.
pub enum IntentState {
    Scheduled,
    Completed,
    Failed { error: String },
}

#[derive(Debug, Clone)]
/// Lifecycle milestones for the supervised agent task.
pub enum AgentLifecycle {
    Started,
    Restarted { attempt: usize },
    Panicked { attempt: usize },
    Exhausted,
    Stopped,
}

#[derive(Debug, Clone)]
/// Structured notifications agents may emit back to the kernel.
pub enum AgentNotification {
    Message(String),
    Structured { label: String, payload: Value },
}

#[derive(Debug, Clone)]
/// Unified event stream the kernel publishes for downstream consumers.
pub enum KernelEvent {
    IntentState {
        agent_id: AgentId,
        intent_id: Uuid,
        state: IntentState,
    },
    AgentLifecycle {
        agent_id: AgentId,
        lifecycle: AgentLifecycle,
    },
    AgentNotification {
        agent_id: AgentId,
        notification: AgentNotification,
    },
}

#[derive(Debug, Clone)]
/// Tunables that shape kernel scheduling behaviour.
pub struct KernelConfig {
    pub mailbox_capacity: usize,
    pub max_restarts: usize,
    pub restart_backoff: Duration,
    pub event_buffer: usize,
}

impl Default for KernelConfig {
    fn default() -> Self {
        Self {
            mailbox_capacity: 32,
            max_restarts: 3,
            restart_backoff: Duration::from_millis(250),
            event_buffer: 256,
        }
    }
}

#[derive(Debug, Error)]
/// Kernel-level failures reported to callers.
pub enum KernelError {
    #[error("agent '{0}' is already registered")]
    AgentAlreadyRegistered(AgentId),
    #[error("agent '{0}' is not registered")]
    AgentNotRegistered(AgentId),
    #[error("command dispatch failed for agent '{agent_id}': {reason}")]
    DispatchFailed { agent_id: AgentId, reason: String },
}

#[async_trait]
/// Behaviour any managed agent must expose.
pub trait KernelAgent: Send {
    /// Handle the supplied intent and optionally emit notifications via the broker.
    async fn handle_intent(&mut self, intent: IntentEnvelope, broker: &AgentBroker) -> Result<()>;
}

/// Factory that produces instances of a kernel-managed agent.
pub trait AgentFactory: Send + Sync {
    fn agent_id(&self) -> &str;
    fn spawn(&self) -> Box<dyn KernelAgent>;
}

#[derive(Clone)]
/// Capability wrapper handed to agents for emitting events back to the kernel.
pub struct AgentBroker {
    agent_id: AgentId,
    events: broadcast::Sender<KernelEvent>,
}

impl AgentBroker {
    fn new(agent_id: AgentId, events: broadcast::Sender<KernelEvent>) -> Self {
        Self { agent_id, events }
    }

    /// Return the bound agent identifier.
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    /// Publish a structured notification originating from the agent.
    pub fn notify(&self, notification: AgentNotification) {
        let event = KernelEvent::AgentNotification {
            agent_id: self.agent_id.clone(),
            notification,
        };
        if let Err(error) = self.events.send(event) {
            debug!(
                agent = %self.agent_id,
                %error,
                "agent notification dropped due to lack of subscribers"
            );
        }
    }

    /// Update downstream listeners about intent progress.
    pub fn intent_state(&self, intent_id: Uuid, state: IntentState) {
        let event = KernelEvent::IntentState {
            agent_id: self.agent_id.clone(),
            intent_id,
            state,
        };
        if let Err(error) = self.events.send(event) {
            debug!(
                agent = %self.agent_id,
                %error,
                "intent state update dropped due to lack of subscribers"
            );
        }
    }
}

struct AgentHandle {
    command_tx: mpsc::Sender<KernelCommand>,
    supervisor: tokio::task::JoinHandle<()>,
}

/// Micro-kernel responsible for supervising agents and relaying messages.
pub struct AgentKernel {
    config: KernelConfig,
    events: broadcast::Sender<KernelEvent>,
    agents: HashMap<AgentId, AgentHandle>,
}

impl AgentKernel {
    /// Construct a kernel using the provided configuration.
    pub fn new(config: KernelConfig) -> Self {
        let (events, _) = broadcast::channel(config.event_buffer);
        Self {
            config,
            events,
            agents: HashMap::new(),
        }
    }

    /// Subscribe to kernel events via a broadcast receiver.
    pub fn subscribe(&self) -> broadcast::Receiver<KernelEvent> {
        self.events.subscribe()
    }

    /// Register and spawn an agent, wiring it into the scheduler.
    pub fn register_agent(&mut self, factory: Arc<dyn AgentFactory>) -> Result<(), KernelError> {
        let agent_id = factory.agent_id().to_string();
        if self.agents.contains_key(&agent_id) {
            return Err(KernelError::AgentAlreadyRegistered(agent_id));
        }

        let (command_tx, command_rx) = mpsc::channel(self.config.mailbox_capacity);
        let events = self.events.clone();
        let config = self.config.clone();
        let id = agent_id.clone();
        let supervisor = tokio::spawn(async move {
            supervise_agent(id, factory, command_rx, events, config).await;
        });

        self.agents.insert(
            agent_id,
            AgentHandle {
                command_tx,
                supervisor,
            },
        );
        Ok(())
    }

    /// Emit an agent notification directly onto the kernel event bus.
    pub fn emit_notification(&self, agent_id: &str, notification: AgentNotification) {
        if let Err(error) = self.events.send(KernelEvent::AgentNotification {
            agent_id: agent_id.to_string(),
            notification,
        }) {
            debug!(%error, "direct notification dropped due to lack of subscribers");
        }
    }

    /// Dispatch an intent to the targeted agent.
    pub async fn dispatch_intent(
        &self,
        agent_id: &str,
        intent: IntentEnvelope,
    ) -> Result<(), KernelError> {
        let handle = self
            .agents
            .get(agent_id)
            .ok_or_else(|| KernelError::AgentNotRegistered(agent_id.to_string()))?;
        handle
            .command_tx
            .send(KernelCommand::Dispatch(intent))
            .await
            .map_err(|err| KernelError::DispatchFailed {
                agent_id: agent_id.to_string(),
                reason: err.to_string(),
            })
    }

    /// Request graceful shutdown of the targeted agent.
    pub async fn shutdown_agent(&self, agent_id: &str) -> Result<(), KernelError> {
        let handle = self
            .agents
            .get(agent_id)
            .ok_or_else(|| KernelError::AgentNotRegistered(agent_id.to_string()))?;
        handle
            .command_tx
            .send(KernelCommand::Shutdown)
            .await
            .map_err(|err| KernelError::DispatchFailed {
                agent_id: agent_id.to_string(),
                reason: err.to_string(),
            })
    }
}

async fn supervise_agent(
    agent_id: AgentId,
    factory: Arc<dyn AgentFactory>,
    mut command_rx: mpsc::Receiver<KernelCommand>,
    events: broadcast::Sender<KernelEvent>,
    config: KernelConfig,
) {
    let mut restart_attempt = 0usize;

    loop {
        let mut agent = factory.spawn();
        let broker = AgentBroker::new(agent_id.clone(), events.clone());

        announce_lifecycle(
            &events,
            &agent_id,
            if restart_attempt == 0 {
                AgentLifecycle::Started
            } else {
                AgentLifecycle::Restarted {
                    attempt: restart_attempt,
                }
            },
        );

        let run_future = run_agent(&mut *agent, &mut command_rx, &broker);
        let guarded = AssertUnwindSafe(run_future).catch_unwind().await;

        match guarded {
            Ok(Ok(())) => {
                announce_lifecycle(&events, &agent_id, AgentLifecycle::Stopped);
                break;
            }
            Ok(Err(err)) => {
                error!(
                    agent = %agent_id,
                    error = %err,
                    "agent loop terminated with error"
                );
                announce_lifecycle(&events, &agent_id, AgentLifecycle::Exhausted);
                break;
            }
            Err(panic) => {
                restart_attempt += 1;
                warn!(
                    agent = %agent_id,
                    attempt = restart_attempt,
                    "agent task panicked, scheduling restart"
                );
                announce_lifecycle(
                    &events,
                    &agent_id,
                    AgentLifecycle::Panicked {
                        attempt: restart_attempt,
                    },
                );
                if restart_attempt > config.max_restarts {
                    error!(
                        agent = %agent_id,
                        max_restarts = config.max_restarts,
                        "agent reached panic threshold, marking exhausted"
                    );
                    announce_lifecycle(&events, &agent_id, AgentLifecycle::Exhausted);
                    let reason = panic_payload_to_string(panic.as_ref());
                    broker.notify(AgentNotification::Message(format!(
                        "agent exhausted after panic: {reason}"
                    )));
                    break;
                }
                sleep(config.restart_backoff).await;
                continue;
            }
        }
    }
}

async fn run_agent(
    agent: &mut dyn KernelAgent,
    command_rx: &mut mpsc::Receiver<KernelCommand>,
    broker: &AgentBroker,
) -> Result<()> {
    while let Some(command) = command_rx.recv().await {
        match command {
            KernelCommand::Dispatch(intent) => {
                let intent_id = intent.id;
                broker.intent_state(intent_id, IntentState::Scheduled);
                match agent.handle_intent(intent, broker).await {
                    Ok(()) => broker.intent_state(intent_id, IntentState::Completed),
                    Err(err) => broker.intent_state(
                        intent_id,
                        IntentState::Failed {
                            error: err.to_string(),
                        },
                    ),
                }
            }
            KernelCommand::Shutdown => {
                info!(agent = %broker.agent_id(), "received shutdown signal");
                break;
            }
        }
    }
    Ok(())
}

fn panic_payload_to_string(payload: &(dyn std::any::Any + Send + 'static)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic".to_string()
    }
}

fn announce_lifecycle(
    events: &broadcast::Sender<KernelEvent>,
    agent_id: &str,
    lifecycle: AgentLifecycle,
) {
    let agent_id = agent_id.to_string();
    if let Err(error) = events.send(KernelEvent::AgentLifecycle {
        agent_id,
        lifecycle,
    }) {
        debug!(%error, "lifecycle event dropped due to lack of subscribers");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::BindrMode;
    use serde_json::json;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tokio::{sync::Mutex, task::yield_now, time::timeout};

    struct TestAgentFactory {
        id: &'static str,
        invocations: Arc<Mutex<Vec<Uuid>>>,
        panic_on_first: bool,
        panic_flag: Arc<AtomicBool>,
    }

    impl TestAgentFactory {
        fn new(id: &'static str, invocations: Arc<Mutex<Vec<Uuid>>>, panic_on_first: bool) -> Self {
            Self {
                id,
                invocations,
                panic_on_first,
                panic_flag: Arc::new(AtomicBool::new(false)),
            }
        }
    }

    struct TestAgent {
        invocations: Arc<Mutex<Vec<Uuid>>>,
        panic_on_first: bool,
        panic_flag: Arc<AtomicBool>,
    }

    #[async_trait]
    impl KernelAgent for TestAgent {
        async fn handle_intent(
            &mut self,
            intent: IntentEnvelope,
            broker: &AgentBroker,
        ) -> Result<()> {
            if self.panic_on_first && !self.panic_flag.swap(true, Ordering::SeqCst) {
                panic!("synthetic panic");
            }

            {
                let mut guard = self.invocations.lock().await;
                guard.push(intent.id);
            }

            broker.notify(AgentNotification::Message("handled".to_string()));
            broker.notify(AgentNotification::Structured {
                label: "intent".to_string(),
                payload: json!({ "intent_id": intent.id.to_string() }),
            });
            Ok(())
        }
    }

    impl AgentFactory for TestAgentFactory {
        fn agent_id(&self) -> &str {
            self.id
        }

        fn spawn(&self) -> Box<dyn KernelAgent> {
            Box::new(TestAgent {
                invocations: Arc::clone(&self.invocations),
                panic_on_first: self.panic_on_first,
                panic_flag: Arc::clone(&self.panic_flag),
            })
        }
    }

    #[tokio::test]
    async fn kernel_handles_dispatch_and_shutdown() {
        let mut kernel = AgentKernel::new(KernelConfig::default());
        let mut events = kernel.subscribe();

        let invocations = Arc::new(Mutex::new(Vec::new()));
        let factory: Arc<dyn AgentFactory> = Arc::new(TestAgentFactory::new(
            "mock",
            Arc::clone(&invocations),
            false,
        ));

        kernel.register_agent(Arc::clone(&factory)).unwrap();
        assert!(matches!(
            kernel.register_agent(Arc::clone(&factory)),
            Err(KernelError::AgentAlreadyRegistered(_))
        ));

        let decision = IntentDecision::new(BindrMode::Brainstorm, 0.9, "demo".to_string());
        let payload = IntentPayload::new("demo".to_string(), None);
        assert!(matches!(
            kernel
                .dispatch_intent(
                    "missing",
                    IntentEnvelope::new(decision.clone(), payload.clone())
                )
                .await,
            Err(KernelError::AgentNotRegistered(_))
        ));

        let envelope = IntentEnvelope::new(decision, payload.clone());
        let intent_id = envelope.id;
        kernel
            .dispatch_intent("mock", envelope)
            .await
            .expect("intent routed");

        let mut saw_completed = false;
        let mut saw_message = false;
        let mut saw_structured = false;
        for _ in 0..16 {
            let event = timeout(Duration::from_secs(1), events.recv())
                .await
                .expect("event available")
                .expect("channel open");
            match event {
                KernelEvent::IntentState {
                    agent_id,
                    intent_id: emitted_id,
                    state,
                } => {
                    assert_eq!(agent_id, "mock");
                    if emitted_id == intent_id && matches!(state, IntentState::Completed) {
                        saw_completed = true;
                    }
                }
                KernelEvent::AgentNotification { notification, .. } => match notification {
                    AgentNotification::Message(_) => saw_message = true,
                    AgentNotification::Structured { .. } => saw_structured = true,
                },
                KernelEvent::AgentLifecycle { .. } => {}
            }

            if saw_completed && saw_message && saw_structured {
                break;
            }
        }

        assert!(saw_completed);
        assert!(saw_message);
        assert!(saw_structured);

        kernel
            .shutdown_agent("mock")
            .await
            .expect("shutdown succeeds");
        yield_now().await;

        let decision = IntentDecision::new(BindrMode::Brainstorm, 0.9, "retry".to_string());
        let payload = IntentPayload::new("retry".to_string(), None);
        assert!(matches!(
            kernel
                .dispatch_intent("mock", IntentEnvelope::new(decision, payload))
                .await,
            Err(KernelError::DispatchFailed { .. })
        ));

        let recorded = invocations.lock().await;
        assert_eq!(recorded.len(), 1);
    }

    #[tokio::test]
    async fn kernel_restarts_panicked_agent() {
        let config = KernelConfig {
            max_restarts: 1,
            restart_backoff: Duration::from_millis(10),
            ..KernelConfig::default()
        };

        let mut kernel = AgentKernel::new(config);
        let mut events = kernel.subscribe();

        let invocations = Arc::new(Mutex::new(Vec::new()));
        let factory: Arc<dyn AgentFactory> = Arc::new(TestAgentFactory::new(
            "panic",
            Arc::clone(&invocations),
            true,
        ));

        kernel.register_agent(Arc::clone(&factory)).unwrap();

        let decision = IntentDecision::new(BindrMode::Brainstorm, 0.9, "boom".to_string());
        let payload = IntentPayload::new("boom".to_string(), None);
        let envelope = IntentEnvelope::new(decision, payload);
        kernel
            .dispatch_intent("panic", envelope)
            .await
            .expect("intent routed");

        let mut saw_panicked = false;
        let mut saw_restarted = false;
        for _ in 0..8 {
            let event = timeout(Duration::from_secs(1), events.recv())
                .await
                .expect("event available")
                .expect("channel open");
            if let KernelEvent::AgentLifecycle { lifecycle, .. } = event {
                match lifecycle {
                    AgentLifecycle::Panicked { .. } => saw_panicked = true,
                    AgentLifecycle::Restarted { .. } => {
                        saw_restarted = true;
                        break;
                    }
                    _ => {}
                }
            }
        }

        assert!(saw_panicked);
        assert!(saw_restarted);

        let decision = IntentDecision::new(BindrMode::Brainstorm, 0.9, "recover".to_string());
        let payload = IntentPayload::new("recover".to_string(), None);
        let envelope = IntentEnvelope::new(decision, payload);
        kernel
            .dispatch_intent("panic", envelope)
            .await
            .expect("intent routed after restart");

        let mut saw_completion = false;
        for _ in 0..8 {
            let event = timeout(Duration::from_secs(1), events.recv())
                .await
                .expect("event available")
                .expect("channel open");
            if let KernelEvent::IntentState { state, .. } = event {
                if matches!(state, IntentState::Completed) {
                    saw_completion = true;
                    break;
                }
            }
        }

        assert!(saw_completion);

        kernel
            .shutdown_agent("panic")
            .await
            .expect("shutdown succeeds");

        let recorded = invocations.lock().await;
        assert_eq!(recorded.len(), 1);
    }
}
