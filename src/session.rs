use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

use crate::config::Config;
use crate::events::{BindrMode, ProjectState, SessionInfo, ConversationEntry, ConversationRole};

/// Session manager for handling project state and persistence
#[derive(Clone)]
pub struct SessionManager {
    config: Config,
    current_session: Option<ActiveSession>,
    sessions: HashMap<String, SessionInfo>,
}

/// Active session with runtime state
#[derive(Clone)]
pub struct ActiveSession {
    #[allow(dead_code)]
    pub session_id: String,
    #[allow(dead_code)]
    pub project_state: ProjectState,
    #[allow(dead_code)]
    pub is_dirty: bool,
    #[allow(dead_code)]
    pub last_save: DateTime<Utc>,
}

impl SessionManager {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            current_session: None,
            sessions: HashMap::new(),
        }
    }
    
    /// Load all available sessions
    pub fn load_sessions(&mut self) -> Result<()> {
        let sessions_dir = self.config.bindr_home.join("sessions");
        if !sessions_dir.exists() {
            fs::create_dir_all(&sessions_dir)
                .context("Failed to create sessions directory")?;
            return Ok(());
        }
        
        let entries = fs::read_dir(&sessions_dir)
            .context("Failed to read sessions directory")?;
        
        for entry in entries {
            let entry = entry.context("Failed to read directory entry")?;
            let path = entry.path();
            
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(session_info) = serde_json::from_str::<SessionInfo>(&content) {
                        self.sessions.insert(session_info.session_id.clone(), session_info);
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Create a new project and session
    #[allow(dead_code)]
    pub fn create_project(&mut self, name: String, project_path: PathBuf) -> Result<String> {
        let session_id = Uuid::new_v4().to_string();
        let now = Utc::now();
        
        // Create project directory
        fs::create_dir_all(&project_path)
            .context("Failed to create project directory")?;
        
        // Create initial bindr.md
        let bindr_md_path = project_path.join("bindr.md");
        let initial_content = format!(
            "# Project: {}\n\n## Status\n- Mode: Brainstorm\n- Created: {}\n\n## Notes\n*Project is in brainstorm phase*\n",
            name, now.to_rfc3339()
        );
        fs::write(&bindr_md_path, &initial_content)
            .context("Failed to create initial bindr.md")?;
        
        // Create project state
        let project_state = ProjectState {
            name: name.clone(),
            path: project_path,
            current_mode: BindrMode::Brainstorm,
            created_at: now.to_rfc3339(),
            last_modified: now.to_rfc3339(),
            bindr_md_content: initial_content,
            conversation_history: Vec::new(),
            conversation_count: 0,
            last_activity: now,
        };
        
        // Create session info
        let session_info = SessionInfo {
            project_name: name,
            current_mode: BindrMode::Brainstorm,
            session_id: session_id.clone(),
            created_at: now,
            last_activity: now,
        };
        
        // Save session info
        self.save_session_info(&session_info)?;
        
        // Create active session
        let active_session = ActiveSession {
            session_id: session_id.clone(),
            project_state,
            is_dirty: false,
            last_save: now,
        };
        
        self.current_session = Some(active_session);
        self.sessions.insert(session_id.clone(), session_info);
        
        Ok(session_id)
    }
    
    /// Open an existing project
    pub fn open_project(&mut self, name: &str) -> Result<String> {
        // Find session for this project
        let session_info = self.sessions.values()
            .find(|s| s.project_name == name)
            .ok_or_else(|| anyhow::anyhow!("Project '{}' not found", name))?
            .clone();
        
        // Load project state
        let project_state = self.load_project_state(&session_info)?;
        
        // Create active session
        let active_session = ActiveSession {
            session_id: session_info.session_id.clone(),
            project_state,
            is_dirty: false,
            last_save: Utc::now(),
        };
        
        self.current_session = Some(active_session);
        
        Ok(session_info.session_id)
    }
    
    /// Get current session
    #[allow(dead_code)]
    pub fn current_session(&self) -> Option<&ActiveSession> {
        self.current_session.as_ref()
    }
    
    /// Get current session mutably
    #[allow(dead_code)]
    pub fn current_session_mut(&mut self) -> Option<&mut ActiveSession> {
        self.current_session.as_mut()
    }
    
    /// Save current session
    #[allow(dead_code)]
    pub fn save_current_session(&mut self) -> Result<()> {
        // Extract data from current session to avoid borrow checker issues
        let (project_state, session_id, current_mode) = if let Some(session) = &self.current_session {
            (
                session.project_state.clone(),
                session.session_id.clone(),
                session.project_state.current_mode,
            )
        } else {
            return Ok(());
        };
        
        // Save project state
        self.save_project_state(&project_state)?;
        
        // Update session info
        if let Some(session_info) = self.sessions.get_mut(&session_id) {
            session_info.last_activity = Utc::now();
            session_info.current_mode = current_mode;
            let session_info_clone = session_info.clone();
            self.save_session_info(&session_info_clone)?;
        }
        
        // Update session state
        if let Some(session) = &mut self.current_session {
            session.is_dirty = false;
            session.last_save = Utc::now();
        }
        
        Ok(())
    }
    
    /// Add conversation entry to current session
    #[allow(dead_code)]
    pub fn add_conversation_entry(&mut self, role: ConversationRole, content: String, mode: BindrMode) -> Result<()> {
        if let Some(session) = &mut self.current_session {
            let entry = ConversationEntry {
                mode,
                role,
                content,
                timestamp: Utc::now(),
            };
            
            session.project_state.conversation_history.push(entry);
            session.project_state.last_modified = Utc::now().to_rfc3339();
            session.is_dirty = true;
        }
        Ok(())
    }
    
    /// Switch mode in current session
    #[allow(dead_code)]
    pub fn switch_mode(&mut self, mode: BindrMode) -> Result<()> {
        if let Some(session) = &mut self.current_session {
            session.project_state.current_mode = mode;
            session.project_state.last_modified = Utc::now().to_rfc3339();
            session.is_dirty = true;
        }
        Ok(())
    }
    
    /// Get all available sessions
    pub fn list_sessions(&self) -> Vec<&SessionInfo> {
        self.sessions.values().collect()
    }
    
    /// Load project state from disk
    fn load_project_state(&self, session_info: &SessionInfo) -> Result<ProjectState> {
        let state_path = self.config.bindr_home
            .join("projects")
            .join(&session_info.project_name)
            .join("state.json");
        
        if state_path.exists() {
            let content = fs::read_to_string(&state_path)
                .context("Failed to read project state")?;
            serde_json::from_str(&content)
                .context("Failed to parse project state")
        } else {
            // Create default state if not found
            Ok(ProjectState {
                name: session_info.project_name.clone(),
                path: self.config.projects_dir.join(&session_info.project_name),
                current_mode: session_info.current_mode,
                created_at: session_info.created_at.to_rfc3339(),
                last_modified: session_info.last_activity.to_rfc3339(),
                bindr_md_content: String::new(),
                conversation_history: Vec::new(),
                conversation_count: 0,
                last_activity: session_info.last_activity,
            })
        }
    }
    
    /// Save project state to disk
    #[allow(dead_code)]
    fn save_project_state(&self, project_state: &ProjectState) -> Result<()> {
        let project_dir = self.config.projects_dir.join(&project_state.name);
        fs::create_dir_all(&project_dir)
            .context("Failed to create project directory")?;
        
        // Save state.json
        let state_path = project_dir.join("state.json");
        let content = serde_json::to_string_pretty(project_state)
            .context("Failed to serialize project state")?;
        fs::write(&state_path, content)
            .context("Failed to write project state")?;
        
        // Save bindr.md
        let bindr_md_path = project_dir.join("bindr.md");
        fs::write(&bindr_md_path, &project_state.bindr_md_content)
            .context("Failed to write bindr.md")?;
        
        Ok(())
    }
    
    /// Save session info to disk
    #[allow(dead_code)]
    fn save_session_info(&self, session_info: &SessionInfo) -> Result<()> {
        let sessions_dir = self.config.bindr_home.join("sessions");
        fs::create_dir_all(&sessions_dir)
            .context("Failed to create sessions directory")?;
        
        let session_path = sessions_dir.join(format!("{}.json", session_info.session_id));
        let content = serde_json::to_string_pretty(session_info)
            .context("Failed to serialize session info")?;
        fs::write(&session_path, content)
            .context("Failed to write session info")?;
        
        Ok(())
    }
}
