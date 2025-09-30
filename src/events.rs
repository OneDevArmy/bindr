use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Internal application events for coordinating between components
#[derive(Debug, Clone)]
pub enum AppEvent {
    /// Start a new brainstorming session
    StartBrainstorm { initial_prompt: Option<String> },
    
    /// Switch to a different mode (brainstorm, plan, execute, document)
    SwitchMode { mode: BindrMode },
    
    /// Create a new project
    CreateProject { name: String, path: PathBuf },
    
    /// Open an existing project
    OpenProject { name: String },
    
    /// Save current session state
    SaveSession,
    
    /// Load session from file
    LoadSession { path: PathBuf },
    
    /// Update API key configuration
    UpdateApiKey { key: String },
    
    /// LLM response received
    LlmResponse { content: String, mode: BindrMode },
    
    /// Request to exit the application
    ExitRequest,
    
    /// Show error message
    ShowError { message: String },
    
    /// Show info message
    ShowInfo { message: String },
}

/// TUI-specific events (keyboard, mouse, etc.)
#[derive(Debug, Clone)]
pub enum TuiEvent {
    /// Key press event
    Key(crossterm::event::KeyEvent),
    
    /// Mouse event
    Mouse(crossterm::event::MouseEvent),
    
    /// Paste event
    Paste(String),
    
    /// Terminal resize
    Resize(u16, u16),
    
    /// Draw/redraw request
    Draw,
}

/// Bindr workflow modes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    
    pub fn description(&self) -> &'static str {
        match self {
            BindrMode::Brainstorm => "Explore ideas and define your project",
            BindrMode::Plan => "Structure your project and create roadmap",
            BindrMode::Execute => "Implement your project based on the plan",
            BindrMode::Document => "Document what was built and create changelog",
        }
    }
    
    pub fn next_mode(&self) -> Option<BindrMode> {
        match self {
            BindrMode::Brainstorm => Some(BindrMode::Plan),
            BindrMode::Plan => Some(BindrMode::Execute),
            BindrMode::Execute => Some(BindrMode::Document),
            BindrMode::Document => None,
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
}

/// Individual conversation entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationEntry {
    pub mode: BindrMode,
    pub role: ConversationRole,
    pub content: String,
    pub timestamp: String,
}

/// Role in conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConversationRole {
    User,
    Assistant,
    System,
}

/// Session information for resuming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub project_name: String,
    pub current_mode: BindrMode,
    pub session_id: String,
    pub created_at: String,
    pub last_activity: String,
}
