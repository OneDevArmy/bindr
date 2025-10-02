use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;
use dirs;

const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api";
const LEGACY_OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// Main application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Selected provider
    pub selected_provider: String,
    
    /// API keys for different providers
    pub api_keys: HashMap<String, String>,
    
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

/// Configuration file structure for TOML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigToml {
    /// Selected provider
    pub selected_provider: Option<String>,
    
    /// Default model to use
    pub default_model: Option<String>,
    
    /// API keys for different providers
    pub api_keys: Option<HashMap<String, String>>,
    
    /// Model provider configuration
    pub model_providers: Option<HashMap<String, ModelProviderToml>>,
    
    /// UI preferences
    pub ui: Option<UiConfigToml>,
}

/// Model provider configuration for TOML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProviderToml {
    pub name: String,
    pub base_url: String,
    pub api_key_env: Option<String>,
    pub models: Vec<ModelInfoToml>,
}

/// Model information for TOML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfoToml {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

/// UI configuration for TOML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfigToml {
    pub theme: Option<String>,
    pub show_emojis: Option<bool>,
    pub max_history_lines: Option<usize>,
}

/// Model provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProvider {
    pub name: String,
    pub base_url: String,
    pub api_key_env: Option<String>,
    pub models: Vec<ModelInfo>,
}

/// Model information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub is_premium: bool,
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
        
        // OpenAI
        model_providers.insert("openai".to_string(), ModelProvider {
            name: "OpenAI".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            api_key_env: Some("OPENAI_API_KEY".to_string()),
            models: vec![
                ModelInfo {
                    id: "gpt-5".to_string(),
                    name: "GPT-5".to_string(),
                    description: "Latest flagship model with advanced reasoning".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "gpt-5-codex".to_string(),
                    name: "GPT-5 Codex".to_string(),
                    description: "Specialized for code generation and analysis".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "gpt-4o".to_string(),
                    name: "GPT-4o".to_string(),
                    description: "Multimodal model with vision capabilities".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "gpt-4o-mini".to_string(),
                    name: "GPT-4o Mini".to_string(),
                    description: "Fast and cost-effective".to_string(),
                    is_premium: false,
                },
                ModelInfo {
                    id: "gpt-3.5-turbo".to_string(),
                    name: "GPT-3.5 Turbo".to_string(),
                    description: "Free tier model".to_string(),
                    is_premium: false,
                },
            ],
        });
        
        // Anthropic
        model_providers.insert("anthropic".to_string(), ModelProvider {
            name: "Anthropic".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            api_key_env: Some("ANTHROPIC_API_KEY".to_string()),
            models: vec![
                ModelInfo {
                    id: "claude-3-5-sonnet-4.5".to_string(),
                    name: "Claude Sonnet 4.5".to_string(),
                    description: "Latest Claude with enhanced reasoning".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "claude-3-opus-4".to_string(),
                    name: "Claude Opus 4".to_string(),
                    description: "Most powerful Claude model".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "claude-3-5-sonnet-20241022".to_string(),
                    name: "Claude 3.5 Sonnet".to_string(),
                    description: "Previous generation flagship".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "claude-3-5-haiku-20241022".to_string(),
                    name: "Claude 3.5 Haiku".to_string(),
                    description: "Fast and efficient".to_string(),
                    is_premium: false,
                },
            ],
        });
        
        // Google
        model_providers.insert("google".to_string(), ModelProvider {
            name: "Google".to_string(),
            base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
            api_key_env: Some("GOOGLE_API_KEY".to_string()),
            models: vec![
                ModelInfo {
                    id: "gemini-2.5-pro".to_string(),
                    name: "Gemini 2.5 Pro".to_string(),
                    description: "Latest flagship with massive context".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "gemini-2.5-flash".to_string(),
                    name: "Gemini 2.5 Flash".to_string(),
                    description: "Fast and efficient latest model".to_string(),
                    is_premium: false,
                },
            ],
        });
        
        // xAI
        model_providers.insert("xai".to_string(), ModelProvider {
            name: "xAI".to_string(),
            base_url: "https://api.x.ai/v1".to_string(),
            api_key_env: Some("XAI_API_KEY".to_string()),
            models: vec![
                ModelInfo {
                    id: "grok-4".to_string(),
                    name: "Grok-4".to_string(),
                    description: "Latest Grok with advanced reasoning".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "grok-3".to_string(),
                    name: "Grok-3".to_string(),
                    description: "Previous generation flagship".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "grok-beta".to_string(),
                    name: "Grok Beta".to_string(),
                    description: "Experimental Grok model".to_string(),
                    is_premium: true,
                },
            ],
        });
        
        // OpenRouter (aggregator)
        model_providers.insert("openrouter".to_string(), ModelProvider {
            name: "OpenRouter".to_string(),
            base_url: OPENROUTER_BASE_URL.to_string(),
            api_key_env: Some("OPENROUTER_API_KEY".to_string()),
            models: vec![
                ModelInfo {
                    id: "openai/gpt-5".to_string(),
                    name: "GPT-5 (via OpenRouter)".to_string(),
                    description: "Latest flagship via OpenRouter".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "openai/gpt-oss-120b:free".to_string(),
                    name: "GPT-OSS 120B (free) (via OpenRouter)".to_string(),
                    description: "Open-source GPT-class model available on the free tier.".to_string(),
                    is_premium: false,
                },
                ModelInfo {
                    id: "anthropic/claude-3-5-sonnet-4.5".to_string(),
                    name: "Claude Sonnet 4.5 (via OpenRouter)".to_string(),
                    description: "Latest Claude via OpenRouter".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "google/gemini-2.5-pro".to_string(),
                    name: "Gemini 2.5 Pro (via OpenRouter)".to_string(),
                    description: "Latest Google model via OpenRouter".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "x-ai/grok-4-fast:free".to_string(),
                    name: "Grok-4-fast (free) (via OpenRouter)".to_string(),
                    description: "Latest Grok via OpenRouter".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "meta-llama/llama-3.1-405b-instruct".to_string(),
                    name: "Llama 3.1 405B (via OpenRouter)".to_string(),
                    description: "Open source powerhouse".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "mistralai/mistral-large".to_string(),
                    name: "Mistral Large (via OpenRouter)".to_string(),
                    description: "Most capable Mistral model".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "z-ai/glm-4.5-air:free".to_string(),
                    name: "Z.AI GLM 4.5 Air (free) (via OpenRouter)".to_string(),
                    description: "Purpose-built for agent-centric applications.".to_string(),
                    is_premium: false,
                },
                ModelInfo {
                    id: "mistralai/mistral-small-3.2-24b-instruct:free".to_string(),
                    name: "Mistral 24B Instruct (free) (via OpenRouter)".to_string(),
                    description: "Mistral optimized for instruction following, repetition reduction, and improved function calling.".to_string(),
                    is_premium: false,
                },
                ModelInfo {
                    id: "custom-model".to_string(),
                    name: "Custom Model".to_string(),
                    description: "Enter any OpenRouter model name".to_string(),
                    is_premium: false,
                },
            ],
        });
        
        // Mistral AI (Direct API)
        model_providers.insert("mistral".to_string(), ModelProvider {
            name: "Mistral AI".to_string(),
            base_url: "https://api.mistral.ai/v1".to_string(),
            api_key_env: Some("MISTRAL_API_KEY".to_string()),
            models: vec![
                ModelInfo {
                    id: "mistral-large-latest".to_string(),
                    name: "Mistral Large".to_string(),
                    description: "Most capable Mistral model".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "mistral-medium-latest".to_string(),
                    name: "Mistral Medium".to_string(),
                    description: "Balanced performance and speed".to_string(),
                    is_premium: false,
                },
                ModelInfo {
                    id: "mistral-small-latest".to_string(),
                    name: "Mistral Small".to_string(),
                    description: "Fast and efficient".to_string(),
                    is_premium: false,
                },
            ],
        });
        
        Config {
            selected_provider: "openai".to_string(),
            api_keys: HashMap::new(),
            default_model: "gpt-4o-mini".to_string(),
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
        self.model_providers.get(&self.selected_provider)
    }
    
    /// Check if API key is configured for current provider
    pub fn has_api_key(&self) -> bool {
        self.has_api_key_for(&self.selected_provider)
    }
    
    /// Check if API key is configured for a specific provider
    pub fn has_api_key_for(&self, provider_id: &str) -> bool {
        self.api_keys.contains_key(provider_id) ||
            self.model_providers
                .get(provider_id)
                .and_then(|p| p.api_key_env.as_ref())
                .map(|env| std::env::var(env).is_ok())
                .unwrap_or(false)
    }
    
    /// Get API key from config or environment
    pub fn get_api_key(&self) -> Option<String> {
        self.get_api_key_for(&self.selected_provider)
    }

    /// Get API key for a specific provider from config or environment
    pub fn get_api_key_for(&self, provider_id: &str) -> Option<String> {
        self.api_keys.get(provider_id).cloned()
            .or_else(|| {
                self.model_providers
                    .get(provider_id)
                    .and_then(|p| p.api_key_env.as_ref())
                    .and_then(|env| std::env::var(env).ok())
            })
    }
    
    /// Update API key for current provider
    pub fn set_api_key(&mut self, provider: String, key: String) {
        self.api_keys.insert(provider, key);
    }
    
    /// Set selected provider
    pub fn set_selected_provider(&mut self, provider: String) {
        self.selected_provider = provider;
    }
    
    /// Get available providers sorted by display name
    pub fn get_providers(&self) -> Vec<(&String, &ModelProvider)> {
        let mut providers: Vec<(&String, &ModelProvider)> = self.model_providers.iter().collect();
        providers.sort_by(|a, b| a.1.name.cmp(&b.1.name));
        providers
    }
    
    /// Set custom model for OpenRouter
    pub fn set_custom_model(&mut self, model_name: String) {
        self.default_model = model_name;
    }
    
    /// Get usage counter info (placeholder for now)
    pub fn get_usage_info(&self) -> (u32, u32) {
        // TODO: Implement actual usage tracking
        (0, 100) // (used, limit)
    }
    
    /// Load configuration from file
    pub fn load() -> Result<Self> {
        let bindr_home = Self::find_bindr_home()?;
        let config_path = bindr_home.join("config.toml");
        
        let config_toml = if config_path.exists() {
            let content = fs::read_to_string(&config_path)
                .with_context(|| format!("Failed to read config from {}", config_path.display()))?;
            toml::from_str::<ConfigToml>(&content)
                .with_context(|| format!("Failed to parse config from {}", config_path.display()))?
        } else {
            ConfigToml::default()
        };
        
        Self::from_config_toml(config_toml, bindr_home)
    }
    
    /// Save configuration to file
    pub fn save(&self) -> Result<()> {
        let config_path = self.bindr_home.join("config.toml");
        let config_toml = self.to_config_toml();
        let toml_content = toml::to_string_pretty(&config_toml)
            .context("Failed to serialize config to TOML")?;
        
        // Ensure directory exists
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory {}", parent.display()))?;
        }
        
        fs::write(&config_path, toml_content)
            .with_context(|| format!("Failed to write config to {}", config_path.display()))?;
        
        Ok(())
    }
    
    /// Find the Bindr home directory
    pub fn find_bindr_home() -> Result<PathBuf> {
        // Honor the `BINDR_HOME` environment variable when it is set
        if let Ok(val) = std::env::var("BINDR_HOME") {
            if !val.is_empty() {
                return Ok(PathBuf::from(val));
            }
        }
        
        let mut p = dirs::home_dir().ok_or_else(|| {
            anyhow::anyhow!("Could not find home directory")
        })?;
        p.push(".bindr");
        Ok(p)
    }
    
    /// Convert from TOML config
    fn from_config_toml(config_toml: ConfigToml, bindr_home: PathBuf) -> Result<Self> {
        let projects_dir = bindr_home.join("projects");
        let cwd = std::env::current_dir()
            .context("Failed to get current working directory")?;
        
        let selected_provider = config_toml.selected_provider
            .unwrap_or_else(|| "openai".to_string());
        
        let default_model = config_toml.default_model
            .unwrap_or_else(|| "gpt-5".to_string());
        
        let api_keys = config_toml.api_keys.unwrap_or_default();
        
        let mut model_providers = if let Some(providers_toml) = config_toml.model_providers {
            providers_toml.into_iter()
                .map(|(id, provider_toml)| {
                    let mut base_url = provider_toml.base_url;
                    if id == "openrouter" {
                        let normalized = base_url.trim_end_matches('/');
                        if normalized == LEGACY_OPENROUTER_BASE_URL {
                            base_url = OPENROUTER_BASE_URL.to_string();
                        }
                    }
                    let models = provider_toml.models.into_iter()
                        .map(|model_toml| ModelInfo {
                            id: model_toml.id,
                            name: model_toml.name,
                            description: model_toml.description.unwrap_or_else(|| "".to_string()),
                            is_premium: false, // Default to false for loaded models
                        })
                        .collect();
                    
                    (id, ModelProvider {
                        name: provider_toml.name,
                        base_url,
                        api_key_env: provider_toml.api_key_env,
                        models,
                    })
                })
                .collect()
        } else {
            Self::create_default_model_providers()
        };

        Self::merge_builtin_provider_catalog(&mut model_providers);
        
        let ui = if let Some(ui_toml) = config_toml.ui {
            UiConfig {
                theme: ui_toml.theme.unwrap_or_else(|| "default".to_string()),
                show_usage_counter: ui_toml.show_emojis.unwrap_or(true),
                auto_save_interval: ui_toml.max_history_lines.unwrap_or(1000) as u64,
            }
        } else {
            UiConfig {
                theme: "default".to_string(),
                show_usage_counter: true,
                auto_save_interval: 30,
            }
        };
        
        Ok(Config {
            selected_provider,
            api_keys,
            default_model,
            model_providers,
            user_instructions: None, // Will be loaded separately
            bindr_home,
            projects_dir,
            cwd,
            ui,
        })
    }

    /// Create default model providers
    fn create_default_model_providers() -> HashMap<String, ModelProvider> {
        let mut model_providers = HashMap::new();
        
        // OpenAI
        model_providers.insert("openai".to_string(), ModelProvider {
            name: "OpenAI".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            api_key_env: Some("OPENAI_API_KEY".to_string()),
            models: vec![
                ModelInfo {
                    id: "gpt-5".to_string(),
                    name: "GPT-5".to_string(),
                    description: "Latest flagship model with advanced reasoning".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "gpt-5-codex".to_string(),
                    name: "GPT-5 Codex".to_string(),
                    description: "Specialized for code generation and analysis".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "gpt-4.1".to_string(),
                    name: "GPT-4.1".to_string(),
                    description: "Previous generation flagship".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "gpt-3.5-turbo".to_string(),
                    name: "GPT-3.5 Turbo".to_string(),
                    description: "Fast and efficient model".to_string(),
                    is_premium: false,
                },
            ],
        });
        
        // Anthropic
        model_providers.insert("anthropic".to_string(), ModelProvider {
            name: "Anthropic".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            api_key_env: Some("ANTHROPIC_API_KEY".to_string()),
            models: vec![
                ModelInfo {
                    id: "claude-3-5-sonnet-4.5".to_string(),
                    name: "Claude Sonnet 4.5".to_string(),
                    description: "Latest flagship with advanced reasoning".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "claude-3-opus-4".to_string(),
                    name: "Claude Opus 4".to_string(),
                    description: "Most capable model for complex tasks".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "claude-3-haiku-3".to_string(),
                    name: "Claude Haiku 3".to_string(),
                    description: "Fast and efficient model".to_string(),
                    is_premium: false,
                },
            ],
        });
        
        // Google
        model_providers.insert("google".to_string(), ModelProvider {
            name: "Google".to_string(),
            base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
            api_key_env: Some("GOOGLE_API_KEY".to_string()),
            models: vec![
                ModelInfo {
                    id: "gemini-2.5-pro".to_string(),
                    name: "Gemini 2.5 Pro".to_string(),
                    description: "Latest flagship with advanced capabilities".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "gemini-2.5-flash".to_string(),
                    name: "Gemini 2.5 Flash".to_string(),
                    description: "Fast and efficient latest model".to_string(),
                    is_premium: false,
                },
            ],
        });
        
        // xAI
        model_providers.insert("xai".to_string(), ModelProvider {
            name: "xAI".to_string(),
            base_url: "https://api.x.ai/v1".to_string(),
            api_key_env: Some("XAI_API_KEY".to_string()),
            models: vec![
                ModelInfo {
                    id: "grok-4".to_string(),
                    name: "Grok-4".to_string(),
                    description: "Latest Grok with advanced reasoning".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "grok-3".to_string(),
                    name: "Grok-3".to_string(),
                    description: "Previous generation flagship".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "grok-beta".to_string(),
                    name: "Grok Beta".to_string(),
                    description: "Experimental Grok model".to_string(),
                    is_premium: true,
                },
            ],
        });

        // OpenRouter (aggregator)
        model_providers.insert("openrouter".to_string(), ModelProvider {
            name: "OpenRouter".to_string(),
            base_url: OPENROUTER_BASE_URL.to_string(),
            api_key_env: Some("OPENROUTER_API_KEY".to_string()),
            models: vec![
                ModelInfo {
                    id: "openai/gpt-5".to_string(),
                    name: "GPT-5 (via OpenRouter)".to_string(),
                    description: "Latest flagship via OpenRouter".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "openai/gpt-oss-120b:free".to_string(),
                    name: "GPT-OSS 120B (free) (via OpenRouter)".to_string(),
                    description: "Open-source GPT-class model available on the free tier.".to_string(),
                    is_premium: false,
                },
                ModelInfo {
                    id: "anthropic/claude-3-5-sonnet-4.5".to_string(),
                    name: "Claude Sonnet 4.5 (via OpenRouter)".to_string(),
                    description: "Latest Claude via OpenRouter".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "google/gemini-2.5-pro".to_string(),
                    name: "Gemini 2.5 Pro (via OpenRouter)".to_string(),
                    description: "Latest Google model via OpenRouter".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "x-ai/grok-4-fast:free".to_string(),
                    name: "Grok-4-fast (free) (via OpenRouter)".to_string(),
                    description: "Latest Grok via OpenRouter".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "meta-llama/llama-3.1-405b-instruct".to_string(),
                    name: "Llama 3.1 405B (via OpenRouter)".to_string(),
                    description: "Open source powerhouse".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "mistralai/mistral-large".to_string(),
                    name: "Mistral Large (via OpenRouter)".to_string(),
                    description: "Most capable Mistral model".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "z-ai/glm-4.5-air:free".to_string(),
                    name: "Z.AI GLM 4.5 Air (free) (via OpenRouter)".to_string(),
                    description: "Purpose-built for agent-centric applications.".to_string(),
                    is_premium: false,
                },
                ModelInfo {
                    id: "mistralai/mistral-small-3.2-24b-instruct:free".to_string(),
                    name: "Mistral 24B Instruct (free) (via OpenRouter)".to_string(),
                    description: "Mistral optimized for instruction following, repetition reduction, and improved function calling.".to_string(),
                    is_premium: false,
                },
                ModelInfo {
                    id: "custom-model".to_string(),
                    name: "Custom Model".to_string(),
                    description: "Enter any OpenRouter model name".to_string(),
                    is_premium: false,
                },
            ],
        });

        // Mistral AI (Direct API)
        model_providers.insert("mistral".to_string(), ModelProvider {
            name: "Mistral AI".to_string(),
            base_url: "https://api.mistral.ai/v1".to_string(),
            api_key_env: Some("MISTRAL_API_KEY".to_string()),
            models: vec![
                ModelInfo {
                    id: "mistral-large".to_string(),
                    name: "Mistral Large".to_string(),
                    description: "Most capable Mistral model".to_string(),
                    is_premium: true,
                },
                ModelInfo {
                    id: "mistral-7b-instruct".to_string(),
                    name: "Mistral 7B Instruct".to_string(),
                    description: "Fast and efficient model".to_string(),
                    is_premium: false,
                },
            ],
        });
        
        model_providers
    }

    /// Ensure built-in providers are present and up-to-date in the configuration
    fn merge_builtin_provider_catalog(model_providers: &mut HashMap<String, ModelProvider>) {
        let builtin = Self::create_default_model_providers();
        for (provider_id, builtin_provider) in builtin {
            model_providers
                .entry(provider_id.clone())
                .and_modify(|existing| {
                    existing.base_url = builtin_provider.base_url.clone();
                    existing.api_key_env = builtin_provider.api_key_env.clone();
                    existing.models = builtin_provider.models.clone();
                })
                .or_insert(builtin_provider);
        }
    }
    
    /// Convert to TOML config
    fn to_config_toml(&self) -> ConfigToml {
        let model_providers = self.model_providers.iter()
            .map(|(id, provider)| {
                let models = provider.models.iter()
                    .map(|model| ModelInfoToml {
                        id: model.id.clone(),
                        name: model.name.clone(),
                        description: Some(model.description.clone()),
                    })
                    .collect();
                
                (id.clone(), ModelProviderToml {
                    name: provider.name.clone(),
                    base_url: provider.base_url.clone(),
                    api_key_env: provider.api_key_env.clone(),
                    models,
                })
            })
            .collect();
        
        ConfigToml {
            selected_provider: Some(self.selected_provider.clone()),
            default_model: Some(self.default_model.clone()),
            api_keys: Some(self.api_keys.clone()),
            model_providers: Some(model_providers),
            ui: Some(UiConfigToml {
                theme: Some(self.ui.theme.clone()),
                show_emojis: Some(self.ui.show_usage_counter),
                max_history_lines: Some(self.ui.auto_save_interval as usize),
            }),
        }
    }
}

impl Default for ConfigToml {
    fn default() -> Self {
        Self {
            selected_provider: None,
            default_model: None,
            api_keys: None,
            model_providers: None,
            ui: None,
        }
    }
}
