use crate::events::BindrMode;
use std::collections::{HashMap, HashSet};

/// Summary of the conversation context relevant for intent routing.
#[derive(Debug, Clone)]
pub struct ConversationContext {
    pub last_mode: Option<BindrMode>,
    pub message_count: usize,
}

impl ConversationContext {
    pub fn new(last_mode: Option<BindrMode>, message_count: usize) -> Self {
        Self {
            last_mode,
            message_count,
        }
    }
}

/// Decision produced by the intent classifier.
#[derive(Debug, Clone)]
pub struct IntentDecision {
    pub mode: BindrMode,
    pub confidence: f32,
    pub rationale: String,
}

impl IntentDecision {
    pub fn new(mode: BindrMode, confidence: f32, rationale: String) -> Self {
        Self {
            mode,
            confidence: confidence.clamp(0.0, 1.0),
            rationale,
        }
    }
}

/// Naive Bayes classifier for mapping user prompts to Bindr modes.
#[derive(Clone)]
pub struct IntentClassifier {
    priors: HashMap<BindrMode, f32>,
    token_log_probs: HashMap<BindrMode, HashMap<String, f32>>,
    default_log_prob: HashMap<BindrMode, f32>,
}

impl IntentClassifier {
    pub fn new() -> Self {
        Self::train(TRAINING_DATA, 1.0)
    }

    fn train(samples: &[(&'static str, BindrMode)], smoothing: f32) -> Self {
        let mut class_counts: HashMap<BindrMode, usize> = HashMap::new();
        let mut token_counts: HashMap<BindrMode, HashMap<String, usize>> = HashMap::new();
        let mut token_totals: HashMap<BindrMode, usize> = HashMap::new();
        let mut vocabulary: HashSet<String> = HashSet::new();

        for (utterance, mode) in samples {
            *class_counts.entry(*mode).or_insert(0) += 1;
            let tokens = tokenize(utterance);
            for token in tokens {
                vocabulary.insert(token.clone());
                token_counts
                    .entry(*mode)
                    .or_insert_with(HashMap::new)
                    .entry(token.clone())
                    .and_modify(|c| *c += 1)
                    .or_insert(1);
                *token_totals.entry(*mode).or_insert(0) += 1;
            }
        }

        let sample_count = samples.len() as f32;
        let vocab_size = vocabulary.len() as f32;

        let priors = class_counts
            .iter()
            .map(|(mode, count)| (*mode, (*count as f32 / sample_count).ln()))
            .collect::<HashMap<_, _>>();

        let mut token_log_probs: HashMap<BindrMode, HashMap<String, f32>> = HashMap::new();
        let mut default_log_prob = HashMap::new();

        for mode in ALL_MODES.iter().copied() {
            let mode_token_counts = token_counts.get(&mode).cloned().unwrap_or_default();
            let total_tokens = token_totals.get(&mode).copied().unwrap_or(0) as f32;
            let denominator = total_tokens + smoothing * vocab_size;

            let mut map = HashMap::new();
            for (token, count) in mode_token_counts {
                let prob = (count as f32 + smoothing) / denominator;
                map.insert(token, prob.ln());
            }

            let unseen_prob = (smoothing / denominator).ln();
            token_log_probs.insert(mode, map);
            default_log_prob.insert(mode, unseen_prob);
        }

        Self {
            priors,
            token_log_probs,
            default_log_prob,
        }
    }

    pub fn infer_intent(&self, ctx: &ConversationContext, input: &str) -> IntentDecision {
        let tokens = tokenize(input);
        let mut scores = HashMap::new();

        for mode in ALL_MODES.iter().copied() {
            let mut score = *self.priors.get(&mode).unwrap_or(&(-3.0));
            let token_map = self.token_log_probs.get(&mode);
            let default = *self.default_log_prob.get(&mode).unwrap_or(&-10.0);

            for token in &tokens {
                score += token_map
                    .and_then(|m| m.get(token))
                    .copied()
                    .unwrap_or(default);
            }

            if ctx.message_count < 3 && mode == BindrMode::Brainstorm {
                score += 0.2;
            }
            if ctx.last_mode == Some(mode) {
                score += 0.1;
            }

            scores.insert(mode, score);
        }

        let (mode, confidence) = normalize_scores(&scores);
        let matching_tokens = explain_tokens(tokens.iter(), mode, &self.token_log_probs);
        let rationale = if matching_tokens.is_empty() {
            format!("predicted {:?} with confidence {:.2}", mode, confidence)
        } else {
            format!(
                "predicted {:?} on tokens: {}",
                mode,
                matching_tokens.join(", ")
            )
        };

        IntentDecision::new(mode, confidence, rationale)
    }
}

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| token.len() > 2)
        .map(String::from)
        .collect()
}

fn normalize_scores(scores: &HashMap<BindrMode, f32>) -> (BindrMode, f32) {
    let max = scores.values().copied().fold(f32::NEG_INFINITY, f32::max);
    let sum_exp: f32 = scores.values().map(|score| (score - max).exp()).sum();

    let mut best_mode = BindrMode::Brainstorm;
    let mut best_score = f32::NEG_INFINITY;
    for (mode, score) in scores {
        if *score > best_score {
            best_score = *score;
            best_mode = *mode;
        }
    }

    let confidence = ((best_score - max).exp()) / sum_exp.max(f32::MIN_POSITIVE);
    (best_mode, confidence)
}

fn explain_tokens<'a>(
    tokens: impl Iterator<Item = &'a String>,
    mode: BindrMode,
    token_log_probs: &HashMap<BindrMode, HashMap<String, f32>>,
) -> Vec<String> {
    let mut unique = HashSet::new();
    if let Some(map) = token_log_probs.get(&mode) {
        for token in tokens {
            if map.contains_key(token) {
                unique.insert(token.clone());
            }
        }
    }
    let mut matches: Vec<String> = unique.into_iter().collect();
    matches.sort();
    matches
}

const TRAINING_DATA: &[(&str, BindrMode)] = &[
    ("let's brainstorm new product ideas", BindrMode::Brainstorm),
    (
        "brainstorm features for the next release",
        BindrMode::Brainstorm,
    ),
    (
        "explore potential directions for the UI",
        BindrMode::Brainstorm,
    ),
    ("research libraries we could use", BindrMode::Brainstorm),
    ("come up with new names", BindrMode::Brainstorm),
    ("start planning the project milestones", BindrMode::Plan),
    ("outline steps for the migration", BindrMode::Plan),
    ("create a roadmap with phases", BindrMode::Plan),
    ("draft the implementation plan", BindrMode::Plan),
    ("organise tasks for sprint planning", BindrMode::Plan),
    ("implement the parser module", BindrMode::Execute),
    ("code the api client", BindrMode::Execute),
    ("refactor this component", BindrMode::Execute),
    ("build the database migrations", BindrMode::Execute),
    ("fix the failing tests", BindrMode::Execute),
    (
        "write documentation for the new feature",
        BindrMode::Document,
    ),
    ("summarise today's work", BindrMode::Document),
    ("prepare release notes", BindrMode::Document),
    ("generate a changelog entry", BindrMode::Document),
    ("document the architecture decisions", BindrMode::Document),
];

const ALL_MODES: [BindrMode; 4] = [
    BindrMode::Brainstorm,
    BindrMode::Plan,
    BindrMode::Execute,
    BindrMode::Document,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executes_on_code_keywords() {
        let router = IntentClassifier::new();
        let ctx = ConversationContext::new(Some(BindrMode::Plan), 5);
        let decision = router.infer_intent(&ctx, "Please implement the parser module");
        assert_eq!(decision.mode, BindrMode::Execute);
        assert!(decision.confidence > 0.4);
    }

    #[test]
    fn prioritises_plan_for_outline_requests() {
        let router = IntentClassifier::new();
        let ctx = ConversationContext::new(None, 1);
        let decision = router.infer_intent(&ctx, "Can you create a project plan?");
        assert_eq!(decision.mode, BindrMode::Plan);
    }
}
