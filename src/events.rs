use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// LLM streaming events
#[derive(Debug, Clone)]
pub enum LlmStreamEvent {
    /// Text delta from streaming response
    TextDelta(String),
    /// Complete response item
    ResponseComplete(String),
    /// Reasoning/thinking content
    ReasoningDelta(String),
    /// Stream completed
    StreamComplete,
    /// Error occurred
    Error(String),
}

/// Bindr workflow modes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BindrMode {
    /// Brainstorming phase - exploring ideas
    Brainstorm,
    /// Planning phase - creating project structure
    Plan,
    /// Execution phase - implementing code
    Execute,
    /// Documentation phase - summarizing results
    Document,
}

impl BindrMode {
    pub fn display_name(&self) -> &'static str {
        match self {
            BindrMode::Brainstorm => "Brainstorm",
            BindrMode::Plan => "Plan",
            BindrMode::Execute => "Execute",
            BindrMode::Document => "Document",
        }
    }
}

/// Project state information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectState {
    pub name: String,
    pub path: PathBuf,
    pub current_mode: BindrMode,
    pub created_at: String,
    pub last_modified: String,
    pub bindr_md_content: String,
    pub conversation_history: Vec<ConversationEntry>,
    pub conversation_count: usize,
    pub last_activity: chrono::DateTime<chrono::Utc>,
}

/// Individual conversation entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationEntry {
    pub mode: BindrMode,
    pub role: ConversationRole,
    pub content: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Role in conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConversationRole {
    User,
    Assistant,
    System,
}

impl std::fmt::Display for ConversationRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConversationRole::User => write!(f, "user"),
            ConversationRole::Assistant => write!(f, "assistant"),
            ConversationRole::System => write!(f, "system"),
        }
    }
}

/// Session information for resuming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub project_name: String,
    pub current_mode: BindrMode,
    pub session_id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_activity: chrono::DateTime<chrono::Utc>,
}
