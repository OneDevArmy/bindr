use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;

/// Main application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// API key for OpenRouter
    pub openrouter_api_key: Option<String>,
    
    /// Default model to use
    pub default_model: String,
    
    /// Model provider configuration
    pub model_providers: HashMap<String, ModelProvider>,
    
    /// User instructions from AGENTS.md
    pub user_instructions: Option<String>,
    
    /// Bindr home directory
    pub bindr_home: PathBuf,
    
    /// Projects directory
    pub projects_dir: PathBuf,
    
    /// Current working directory
    pub cwd: PathBuf,
    
    /// UI preferences
    pub ui: UiConfig,
}

/// Model provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProvider {
    pub name: String,
    pub base_url: String,
    pub api_key_env: Option<String>,
    pub models: Vec<String>,
}

/// UI configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub theme: String,
    pub show_usage_counter: bool,
    pub auto_save_interval: u64, // seconds
}

impl Default for Config {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
        let bindr_home = home.join(".bindr");
        let projects_dir = bindr_home.join("projects");
        
        let mut model_providers = HashMap::new();
        model_providers.insert("openrouter".to_string(), ModelProvider {
            name: "OpenRouter".to_string(),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            api_key_env: Some("OPENROUTER_API_KEY".to_string()),
            models: vec![
                "openai/gpt-4o".to_string(),
                "openai/gpt-4o-mini".to_string(),
                "anthropic/claude-3.5-sonnet".to_string(),
                "anthropic/claude-3-haiku".to_string(),
            ],
        });
        
        Config {
            openrouter_api_key: None,
            default_model: "openai/gpt-4o-mini".to_string(),
            model_providers,
            user_instructions: None,
            bindr_home,
            projects_dir,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            ui: UiConfig {
                theme: "dark".to_string(),
                show_usage_counter: true,
                auto_save_interval: 30,
            },
        }
    }
}

impl Config {
    /// Load configuration from file and merge with AGENTS.md
    pub fn load() -> Result<Self> {
        let home = dirs::home_dir().context("Could not find home directory")?;
        let bindr_home = home.join(".bindr");
        let config_path = bindr_home.join("config.toml");
        
        // Ensure bindr directory exists
        fs::create_dir_all(&bindr_home)
            .context("Failed to create .bindr directory")?;
        
        let mut config = if config_path.exists() {
            let content = fs::read_to_string(&config_path)
                .context("Failed to read config file")?;
            toml::from_str(&content)
                .context("Failed to parse config file")?
        } else {
            Config::default()
        };
        
        // Load AGENTS.md if it exists
        config.user_instructions = Self::load_agents_md(&config.cwd)?;
        
        // Update paths
        config.bindr_home = bindr_home.clone();
        config.projects_dir = bindr_home.join("projects");
        config.cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        
        Ok(config)
    }
    
    /// Save configuration to file
    pub fn save(&self) -> Result<()> {
        let config_path = self.bindr_home.join("config.toml");
        let content = toml::to_string_pretty(self)
            .context("Failed to serialize config")?;
        fs::write(&config_path, content)
            .context("Failed to write config file")?;
        Ok(())
    }
    
    /// Load AGENTS.md from multiple locations (like Codex does)
    fn load_agents_md(cwd: &Path) -> Result<Option<String>> {
        // Check multiple locations in order of precedence
        let locations = vec![
            cwd.join("AGENTS.md"),                    // Current directory
            cwd.join("..").join("AGENTS.md"),         // Parent directory
            dirs::home_dir().unwrap_or_default().join(".bindr").join("AGENTS.md"), // Global
        ];
        
        for path in locations {
            if path.exists() {
                let content = fs::read_to_string(&path)
                    .context("Failed to read AGENTS.md")?;
                if !content.trim().is_empty() {
                    return Ok(Some(content));
                }
            }
        }
        
        Ok(None)
    }
    
    /// Get the current model provider
    pub fn get_current_provider(&self) -> Option<&ModelProvider> {
        // For now, just return OpenRouter if available
        self.model_providers.get("openrouter")
    }
    
    /// Check if API key is configured
    pub fn has_api_key(&self) -> bool {
        self.openrouter_api_key.is_some() || 
        std::env::var("OPENROUTER_API_KEY").is_ok()
    }
    
    /// Get API key from config or environment
    pub fn get_api_key(&self) -> Option<String> {
        self.openrouter_api_key.clone()
            .or_else(|| std::env::var("OPENROUTER_API_KEY").ok())
    }
    
    /// Update API key
    pub fn set_api_key(&mut self, key: String) {
        self.openrouter_api_key = Some(key);
    }
    
    /// Get usage counter info (placeholder for now)
    pub fn get_usage_info(&self) -> (u32, u32) {
        // TODO: Implement actual usage tracking
        (0, 100) // (used, limit)
    }
}
