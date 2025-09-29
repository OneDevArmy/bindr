use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectMetadata {
    pub name: String,
    pub path: PathBuf,
    pub created_at: String,
    pub current_mode: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BindrConfig {
    pub api_keys: std::collections::HashMap<String, String>,
    pub model_preferences: std::collections::HashMap<String, String>,
}

pub struct StorageManager {
    bindr_dir: PathBuf,
    projects_dir: PathBuf,
    config_path: PathBuf,
}

impl StorageManager {
    pub fn new() -> Result<Self> {
        let home_dir = dirs::home_dir()
            .context("Could not find home directory")?;
        
        let bindr_dir = home_dir.join(".bindr");
        let projects_dir = bindr_dir.join("projects");
        let config_path = bindr_dir.join("config.toml");

        Ok(StorageManager {
            bindr_dir,
            projects_dir,
            config_path,
        })
    }

    pub fn ensure_directories(&self) -> Result<()> {
        fs::create_dir_all(&self.bindr_dir)
            .context("Failed to create .bindr directory")?;
        
        fs::create_dir_all(&self.projects_dir)
            .context("Failed to create projects directory")?;

        Ok(())
    }

    pub fn get_projects(&self) -> Result<Vec<ProjectMetadata>> {
        self.ensure_directories()?;
        
        let mut projects = Vec::new();
        
        if !self.projects_dir.exists() {
            return Ok(projects);
        }

        let entries = fs::read_dir(&self.projects_dir)
            .context("Failed to read projects directory")?;

        for entry in entries {
            let entry = entry.context("Failed to read directory entry")?;
            let project_dir = entry.path();
            
            if project_dir.is_dir() {
                let metadata_path = project_dir.join("metadata.json");
                if metadata_path.exists() {
                    let metadata_content = fs::read_to_string(&metadata_path)
                        .context("Failed to read project metadata")?;
                    
                    let metadata: ProjectMetadata = serde_json::from_str(&metadata_content)
                        .context("Failed to parse project metadata")?;
                    
                    projects.push(metadata);
                }
            }
        }

        Ok(projects)
    }

    pub fn create_project(&self, name: &str, project_path: PathBuf) -> Result<()> {
        self.ensure_directories()?;
        
        let project_dir = self.projects_dir.join(name);
        fs::create_dir_all(&project_dir)
            .context("Failed to create project directory")?;

        let metadata = ProjectMetadata {
            name: name.to_string(),
            path: project_path,
            created_at: chrono::Utc::now().to_rfc3339(),
            current_mode: "brainstorm".to_string(),
        };

        let metadata_path = project_dir.join("metadata.json");
        let metadata_content = serde_json::to_string_pretty(&metadata)
            .context("Failed to serialize project metadata")?;
        
        fs::write(&metadata_path, metadata_content)
            .context("Failed to write project metadata")?;

        // Create initial bindr.md file
        let bindr_md_path = project_dir.join("bindr.md");
        let initial_content = format!("# Project: {}\n\n## Status\n- Mode: Brainstorm\n- Created: {}\n\n## Notes\n*Project is in brainstorm phase*\n", 
            name, metadata.created_at);
        
        fs::write(&bindr_md_path, initial_content)
            .context("Failed to create initial bindr.md")?;

        Ok(())
    }

    pub fn get_project_metadata(&self, name: &str) -> Result<ProjectMetadata> {
        let project_dir = self.projects_dir.join(name);
        let metadata_path = project_dir.join("metadata.json");
        
        let metadata_content = fs::read_to_string(&metadata_path)
            .context("Failed to read project metadata")?;
        
        let metadata: ProjectMetadata = serde_json::from_str(&metadata_content)
            .context("Failed to parse project metadata")?;
        
        Ok(metadata)
    }

    pub fn project_exists(&self, name: &str) -> bool {
        let project_dir = self.projects_dir.join(name);
        project_dir.exists() && project_dir.join("metadata.json").exists()
    }
}
