use crate::config::{Config, ModelInfo};
use crate::events::BindrMode;

/// Requirements used when selecting a model for a request.
#[derive(Debug, Clone)]
pub struct ModelRequest<'a> {
    pub intent: BindrMode,
    pub config: &'a Config,
}

/// Result of the model selection process.
#[derive(Debug, Clone)]
pub struct ModelDecision {
    pub provider_id: String,
    pub model_id: String,
    pub rationale: String,
}

#[derive(Default)]
pub struct ModelSelector;

impl ModelSelector {
    pub fn new() -> Self {
        Self
    }

    pub fn select_model(&self, request: &ModelRequest<'_>) -> Option<ModelDecision> {
        let mut best: Option<(String, String, f32, String)> = None;

        for (provider_id, provider) in request.config.get_providers() {
            if !request.config.has_api_key_for(provider_id) {
                continue;
            }

            for model in &provider.models {
                let score = score_model(request.intent, model);
                let rationale = format!(
                    "intent {:?} → {}::{} (score {:.2})",
                    request.intent, provider.name, model.name, score
                );

                match &mut best {
                    Some(current) if current.2 >= score => {}
                    Some(current) => {
                        current.0 = provider_id.clone();
                        current.1 = model.id.clone();
                        current.2 = score;
                        current.3 = rationale;
                    }
                    None => {
                        best = Some((provider_id.clone(), model.id.clone(), score, rationale));
                    }
                }
            }
        }

        best.map(|(provider_id, model_id, _score, rationale)| ModelDecision {
            provider_id,
            model_id,
            rationale,
        })
    }
}

fn score_model(intent: BindrMode, model: &ModelInfo) -> f32 {
    let base = if model.is_premium { 0.6 } else { 0.8 };

    match intent {
        BindrMode::Brainstorm => base + if model.is_premium { -0.2 } else { 0.1 },
        BindrMode::Plan => base + 0.0,
        BindrMode::Execute => base + if model.is_premium { 0.25 } else { -0.1 },
        BindrMode::Document => base + if model.is_premium { 0.1 } else { 0.0 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_premium_for_execute() {
        let mut config = Config::default();
        config
            .api_keys
            .insert(config.selected_provider.clone(), "test-key".to_string());
        let request = ModelRequest {
            intent: BindrMode::Execute,
            config: &config,
        };
        let selector = ModelSelector::new();
        let decision = selector.select_model(&request);
        assert!(decision.is_some());
        if let Some(choice) = decision {
            let provider_cfg = config
                .model_providers
                .get(&choice.provider_id)
                .expect("provider");
            let model = provider_cfg
                .models
                .iter()
                .find(|m| m.id == choice.model_id)
                .expect("model");
            assert!(model.is_premium);
        }
    }
}
