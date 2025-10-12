use crate::config::Config;
use crate::llm::LlmClient;
use crate::session::SessionManager;

/// Shared runtime state available to all agents.
pub struct RuntimeState {
    pub config: Config,
    pub session_manager: SessionManager,
    pub llm_client: LlmClient,
    pub current_provider: String,
    pub current_model: String,
}

impl RuntimeState {
    pub fn new(config: Config, session_manager: SessionManager) -> Self {
        let llm_client = LlmClient::new(config.clone());
        let current_provider = config.selected_provider.clone();
        let current_model = config.default_model.clone();

        Self {
            config,
            session_manager,
            llm_client,
            current_provider,
            current_model,
        }
    }

    pub fn rebuild_llm(&mut self) {
        self.llm_client = LlmClient::new(self.config.clone());
    }
}
