use crate::config::{Config, ModelProvider as ProviderConfig};
use crate::events::BindrMode;
use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info, trace, warn};

const MAX_RETRIES: usize = 3;
const RETRY_BACKOFF_BASE_MS: u64 = 500;

/// Events emitted during LLM streaming
#[derive(Debug, Clone)]
pub enum LlmEvent {
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

/// Request to send to an LLM provider
#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub messages: Vec<LlmMessage>,
    pub mode: BindrMode,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
}

/// Message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("configuration error: {0}")]
    Configuration(String),
    #[error("http error ({status}): {message}")]
    Http { status: StatusCode, message: String },
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("unexpected response format: {0}")]
    UnexpectedFormat(String),
}

#[derive(Clone)]
pub struct ProviderContext {
    pub client: reqwest::Client,
    pub config: ProviderConfig,
    pub model: String,
    pub api_key: String,
    pub request: LlmRequest,
}

#[async_trait]
pub trait ModelProvider: Send + Sync {
    fn id(&self) -> &'static str;

    async fn stream(
        &self,
        ctx: ProviderContext,
        tx: mpsc::Sender<LlmEvent>,
    ) -> std::result::Result<(), LlmError>;
}

/// Client responsible for dispatching streaming LLM requests
#[derive(Clone)]
pub struct LlmClient {
    config: Config,
    client: reqwest::Client,
    providers: HashMap<String, Arc<dyn ModelProvider>>,
}

impl LlmClient {
    /// Create a new LLM client using the provided configuration.
    pub fn new(config: Config) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("Failed to create HTTP client");

        let mut providers: HashMap<String, Arc<dyn ModelProvider>> = HashMap::new();
        providers.insert("openai".to_string(), Arc::new(OpenAiProvider));
        providers.insert("anthropic".to_string(), Arc::new(AnthropicProvider));
        providers.insert("google".to_string(), Arc::new(GoogleProvider));
        providers.insert("xai".to_string(), Arc::new(XaiProvider));
        providers.insert("openrouter".to_string(), Arc::new(OpenRouterProvider));
        providers.insert("mistral".to_string(), Arc::new(MistralProvider));

        Self {
            config,
            client,
            providers,
        }
    }

    /// Stream a response from the selected LLM provider.
    pub async fn stream_response(
        &self,
        mut request: LlmRequest,
    ) -> Result<mpsc::Receiver<LlmEvent>> {
        let (tx, rx) = mpsc::channel(1000);

        if self.config.get_api_key().is_none() {
            error!(event = "llm_request_failed", reason = "missing_api_key");
            let _ = tx
                .send(LlmEvent::Error(
                    "No API key configured. Please add an API key first.".to_string(),
                ))
                .await;
            return Ok(rx);
        }

        let provider_id = request
            .provider_id
            .clone()
            .unwrap_or_else(|| self.config.selected_provider.clone());
        let provider_key = provider_id.to_lowercase();

        let provider_config = self
            .config
            .model_providers
            .get(&provider_key)
            .cloned()
            .ok_or_else(|| {
                LlmError::Configuration(format!("No provider configured for id {provider_id}"))
            })?;

        let provider_impl = self.providers.get(&provider_key).cloned().ok_or_else(|| {
            LlmError::Configuration(format!("No runtime support for provider {provider_id}"))
        })?;

        let api_key = self.config.get_api_key_for(&provider_key).ok_or_else(|| {
            LlmError::Configuration(format!("No API key configured for provider {provider_id}"))
        })?;

        let model = request
            .model_id
            .clone()
            .unwrap_or_else(|| self.config.default_model.clone());
        if request.model_id.is_none() {
            request.model_id = Some(model.clone());
        }

        let message_count = request.messages.len();
        let temperature = request.temperature.unwrap_or(0.7);
        let max_tokens = request.max_tokens.unwrap_or(4000);
        let mode_label = request.mode.display_name();
        let provider_name = provider_config.name.clone();

        info!(
            event = "llm_api_call",
            provider = provider_name.as_str(),
            provider_id = provider_key.as_str(),
            model = model.as_str(),
            mode = mode_label,
            message_count,
            temperature,
            max_tokens,
            "dispatching streaming request"
        );

        let ctx = ProviderContext {
            client: self.client.clone(),
            config: provider_config,
            model: model.clone(),
            api_key,
            request,
        };

        let provider_task = provider_impl.clone();
        let tx_task = tx.clone();

        tokio::spawn(async move {
            if let Err(err) = dispatch_with_retry(provider_task, ctx, tx_task.clone()).await {
                error!(event = "llm_stream_error", error = %err);
                let _ = tx_task.send(LlmEvent::Error(err.to_string())).await;
                let _ = tx_task.send(LlmEvent::StreamComplete).await;
            }
        });

        Ok(rx)
    }
}

async fn dispatch_with_retry(
    provider: Arc<dyn ModelProvider>,
    ctx: ProviderContext,
    tx: mpsc::Sender<LlmEvent>,
) -> std::result::Result<(), LlmError> {
    let mut attempt = 0usize;

    loop {
        attempt += 1;
        let tx_attempt = tx.clone();
        let ctx_attempt = ctx.clone();

        match provider.stream(ctx_attempt, tx_attempt.clone()).await {
            Ok(()) => return Ok(()),
            Err(err) if attempt < MAX_RETRIES => {
                warn!(
                    event = "llm_stream_retry",
                    provider = provider.id(),
                    attempt,
                    max_attempts = MAX_RETRIES,
                    error = %err,
                    "LLM streaming failed, retrying"
                );
                sleep(retry_delay(attempt)).await;
            }
            Err(err) => {
                error!(
                    event = "llm_stream_failure",
                    provider = provider.id(),
                    attempt,
                    error = %err,
                    "LLM streaming failed after retries"
                );
                let _ = tx.clone().send(LlmEvent::Error(err.to_string())).await;
                let _ = tx.clone().send(LlmEvent::StreamComplete).await;
                return Err(err);
            }
        }
    }
}

fn retry_delay(attempt: usize) -> Duration {
    let exponent = attempt.saturating_sub(1) as u32;
    let factor = 1u64.checked_shl(exponent).unwrap_or(u64::MAX);
    let delay = RETRY_BACKOFF_BASE_MS.saturating_mul(factor).min(30_000);
    Duration::from_millis(delay)
}

struct OpenAiProvider;

#[async_trait]
impl ModelProvider for OpenAiProvider {
    fn id(&self) -> &'static str {
        "openai"
    }

    async fn stream(
        &self,
        ctx: ProviderContext,
        tx: mpsc::Sender<LlmEvent>,
    ) -> std::result::Result<(), LlmError> {
        let ProviderContext {
            client,
            config,
            model,
            api_key,
            request,
        } = ctx;

        let url = join_url(&config.base_url, "chat/completions");
        let temperature = request.temperature.unwrap_or(0.7);
        let max_tokens = request.max_tokens.unwrap_or(4000);

        let payload = json!({
            "model": model,
            "messages": request.messages,
            "stream": true,
            "temperature": temperature,
            "max_tokens": max_tokens
        });

        info!(
            event = "llm_http_request",
            provider = config.name.as_str(),
            provider_id = self.id(),
            model = payload["model"].as_str().unwrap_or_default(),
            url = %url,
            "sending OpenAI streaming request"
        );

        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        let response = ensure_success(response, self.id()).await?;

        debug!(
            event = "llm_http_response",
            provider = config.name.as_str(),
            provider_id = self.id(),
            status = %response.status(),
            "OpenAI response accepted"
        );

        process_sse_stream(response, tx).await
    }
}

struct AnthropicProvider;

#[async_trait]
impl ModelProvider for AnthropicProvider {
    fn id(&self) -> &'static str {
        "anthropic"
    }

    async fn stream(
        &self,
        ctx: ProviderContext,
        tx: mpsc::Sender<LlmEvent>,
    ) -> std::result::Result<(), LlmError> {
        let ProviderContext {
            client,
            config,
            model,
            api_key,
            request,
        } = ctx;

        let url = join_url(&config.base_url, "v1/messages");

        let mut messages = Vec::new();
        let mut system = String::new();

        for msg in request.messages {
            if msg.role == "system" {
                system = msg.content;
            } else {
                messages.push(json!({
                    "role": msg.role,
                    "content": [{"type": "text", "text": msg.content}]
                }));
            }
        }

        let temperature = request.temperature.unwrap_or(0.7);
        let max_tokens = request.max_tokens.unwrap_or(4000);

        let mut payload = json!({
            "model": model,
            "messages": messages,
            "stream": true,
            "temperature": temperature,
            "max_tokens": max_tokens
        });

        if !system.is_empty() {
            payload["system"] = Value::String(system);
        }

        info!(
            event = "llm_http_request",
            provider = config.name.as_str(),
            provider_id = self.id(),
            model = payload["model"].as_str().unwrap_or_default(),
            url = %url,
            "sending Anthropic streaming request"
        );

        let response = client
            .post(&url)
            .header("x-api-key", api_key)
            .header("Content-Type", "application/json")
            .header("anthropic-version", "2023-06-01")
            .json(&payload)
            .send()
            .await?;

        let response = ensure_success(response, self.id()).await?;

        debug!(
            event = "llm_http_response",
            provider = config.name.as_str(),
            provider_id = self.id(),
            status = %response.status(),
            "Anthropic response accepted"
        );

        process_anthropic_stream(response, tx).await
    }
}

struct GoogleProvider;

#[async_trait]
impl ModelProvider for GoogleProvider {
    fn id(&self) -> &'static str {
        "google"
    }

    async fn stream(
        &self,
        ctx: ProviderContext,
        tx: mpsc::Sender<LlmEvent>,
    ) -> std::result::Result<(), LlmError> {
        let ProviderContext {
            client,
            config,
            model,
            api_key,
            request,
        } = ctx;

        let base = config.base_url.trim_end_matches('/');
        let url = format!("{base}/models/{model}:streamGenerateContent?key={api_key}",);

        let mut contents = Vec::new();
        let mut system_instruction = String::new();

        for msg in request.messages {
            if msg.role == "system" {
                system_instruction = msg.content;
            } else {
                contents.push(json!({
                    "role": msg.role,
                    "parts": [{"text": msg.content}]
                }));
            }
        }

        let temperature = request.temperature.unwrap_or(0.7);
        let max_tokens = request.max_tokens.unwrap_or(4000);
        let mut payload = json!({
            "contents": contents,
            "generationConfig": {
                "temperature": temperature,
                "maxOutputTokens": max_tokens
            }
        });

        if !system_instruction.is_empty() {
            payload["systemInstruction"] = json!({
                "parts": [{"text": system_instruction}]
            });
        }

        info!(
            event = "llm_http_request",
            provider = config.name.as_str(),
            provider_id = self.id(),
            model = model.as_str(),
            url = %url,
            "sending Google Gemini streaming request"
        );

        let response = client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        let response = ensure_success(response, self.id()).await?;

        debug!(
            event = "llm_http_response",
            provider = config.name.as_str(),
            provider_id = self.id(),
            status = %response.status(),
            "Google Gemini response accepted"
        );

        process_google_stream(response, tx).await
    }
}

struct XaiProvider;

#[async_trait]
impl ModelProvider for XaiProvider {
    fn id(&self) -> &'static str {
        "xai"
    }

    async fn stream(
        &self,
        ctx: ProviderContext,
        tx: mpsc::Sender<LlmEvent>,
    ) -> std::result::Result<(), LlmError> {
        let ProviderContext {
            client,
            config,
            model,
            api_key,
            request,
        } = ctx;

        let url = join_url(&config.base_url, "v1/chat/completions");
        let temperature = request.temperature.unwrap_or(0.7);
        let max_tokens = request.max_tokens.unwrap_or(4000);

        let payload = json!({
            "model": model,
            "messages": request.messages,
            "stream": true,
            "temperature": temperature,
            "max_tokens": max_tokens
        });

        info!(
            event = "llm_http_request",
            provider = config.name.as_str(),
            provider_id = self.id(),
            model = payload["model"].as_str().unwrap_or_default(),
            url = %url,
            "sending xAI streaming request"
        );

        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        let response = ensure_success(response, self.id()).await?;

        debug!(
            event = "llm_http_response",
            provider = config.name.as_str(),
            provider_id = self.id(),
            status = %response.status(),
            "xAI response accepted"
        );

        process_sse_stream(response, tx).await
    }
}

struct OpenRouterProvider;

#[async_trait]
impl ModelProvider for OpenRouterProvider {
    fn id(&self) -> &'static str {
        "openrouter"
    }

    async fn stream(
        &self,
        ctx: ProviderContext,
        tx: mpsc::Sender<LlmEvent>,
    ) -> std::result::Result<(), LlmError> {
        let ProviderContext {
            client,
            config,
            model,
            api_key,
            request,
        } = ctx;

        let url = join_url(&config.base_url, "v1/chat/completions");
        let temperature = request.temperature.unwrap_or(0.7);
        let max_tokens = request.max_tokens.unwrap_or(4000);

        let payload = json!({
            "model": model,
            "messages": request.messages,
            "stream": true,
            "temperature": temperature,
            "max_tokens": max_tokens
        });

        info!(
            event = "llm_http_request",
            provider = config.name.as_str(),
            provider_id = self.id(),
            model = payload["model"].as_str().unwrap_or_default(),
            url = %url,
            "sending OpenRouter streaming request"
        );

        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", "https://bindrcli.com")
            .header("X-Title", "Bindr")
            .json(&payload)
            .send()
            .await?;

        let response = ensure_success(response, self.id()).await?;

        debug!(
            event = "llm_http_response",
            provider = config.name.as_str(),
            provider_id = self.id(),
            status = %response.status(),
            "OpenRouter response accepted"
        );

        process_sse_stream(response, tx).await
    }
}

struct MistralProvider;

#[async_trait]
impl ModelProvider for MistralProvider {
    fn id(&self) -> &'static str {
        "mistral"
    }

    async fn stream(
        &self,
        ctx: ProviderContext,
        tx: mpsc::Sender<LlmEvent>,
    ) -> std::result::Result<(), LlmError> {
        let ProviderContext {
            client,
            config,
            model,
            api_key,
            request,
        } = ctx;

        let url = join_url(&config.base_url, "v1/chat/completions");
        let temperature = request.temperature.unwrap_or(0.7);
        let max_tokens = request.max_tokens.unwrap_or(4000);

        let payload = json!({
            "model": model,
            "messages": request.messages,
            "stream": true,
            "temperature": temperature,
            "max_tokens": max_tokens
        });

        info!(
            event = "llm_http_request",
            provider = config.name.as_str(),
            provider_id = self.id(),
            model = payload["model"].as_str().unwrap_or_default(),
            url = %url,
            "sending Mistral streaming request"
        );

        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        let response = ensure_success(response, self.id()).await?;

        debug!(
            event = "llm_http_response",
            provider = config.name.as_str(),
            provider_id = self.id(),
            status = %response.status(),
            "Mistral response accepted"
        );

        process_sse_stream(response, tx).await
    }
}

fn join_url(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    format!("{base}/{path}")
}

async fn ensure_success(
    response: reqwest::Response,
    provider: &str,
) -> std::result::Result<reqwest::Response, LlmError> {
    if response.status().is_success() {
        Ok(response)
    } else {
        let status = response.status();
        let message = match response.text().await {
            Ok(body) if !body.trim().is_empty() => body,
            Ok(_) => format!("{provider} returned empty error body"),
            Err(err) => format!("failed to read error body: {err}"),
        };
        Err(LlmError::Http { status, message })
    }
}

async fn process_sse_stream(
    response: reqwest::Response,
    tx: mpsc::Sender<LlmEvent>,
) -> std::result::Result<(), LlmError> {
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut assistant_text = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        trace!(event = "llm_stream_chunk", bytes = chunk.len());
        let text = String::from_utf8_lossy(&chunk);
        buffer.push_str(&text);
        while let Some(newline_pos) = buffer.find('\n') {
            let line = buffer[..newline_pos].trim().to_string();
            buffer = buffer[newline_pos + 1..].to_string();

            if let Some(data) = line.strip_prefix("data: ").map(str::trim) {
                if data == "[DONE]" {
                    break;
                }

                if let Ok(value) = serde_json::from_str::<Value>(data)
                    && let Some(choice) = value.get("choices").and_then(|choices| choices.get(0))
                {
                    if let Some(delta) = choice.get("delta") {
                        if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                            assistant_text.push_str(content);
                            let _ = tx.send(LlmEvent::TextDelta(content.to_string())).await;
                        }

                        if let Some(reasoning) = delta.get("reasoning").and_then(|r| r.as_str()) {
                            let _ = tx
                                .send(LlmEvent::ReasoningDelta(reasoning.to_string()))
                                .await;
                        }
                    }

                    if choice
                        .get("finish_reason")
                        .and_then(|v| v.as_str())
                        .is_some_and(|finish_reason| {
                            finish_reason == "stop" && !assistant_text.is_empty()
                        })
                    {
                        let _ = tx
                            .send(LlmEvent::ResponseComplete(assistant_text.clone()))
                            .await;
                    }
                }
            }
        }
    }

    if !assistant_text.is_empty() {
        let _ = tx
            .send(LlmEvent::ResponseComplete(assistant_text.clone()))
            .await;
    }
    let _ = tx.send(LlmEvent::StreamComplete).await;
    Ok(())
}

async fn process_anthropic_stream(
    response: reqwest::Response,
    tx: mpsc::Sender<LlmEvent>,
) -> std::result::Result<(), LlmError> {
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut assistant_text = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        trace!(event = "llm_stream_chunk", bytes = chunk.len());
        let text = String::from_utf8_lossy(&chunk);
        buffer.push_str(&text);

        while let Some(newline_pos) = buffer.find('\n') {
            let line = buffer[..newline_pos].trim().to_string();
            buffer = buffer[newline_pos + 1..].to_string();

            if let Some(data) = line.strip_prefix("data: ") {
                let data = data.trim();
                if data == "[DONE]" {
                    if !assistant_text.is_empty() {
                        let _ = tx
                            .send(LlmEvent::ResponseComplete(assistant_text.clone()))
                            .await;
                    }
                    let _ = tx.send(LlmEvent::StreamComplete).await;
                    return Ok(());
                }

                if let Ok(value) = serde_json::from_str::<Value>(data) {
                    if let Some(content_block) = value.get("content_block")
                        && let Some(text) = content_block.get("text").and_then(|t| t.as_str())
                    {
                        assistant_text.push_str(text);
                        let _ = tx.send(LlmEvent::TextDelta(text.to_string())).await;
                    }

                    if value
                        .get("stop_reason")
                        .and_then(|v| v.as_str())
                        .is_some_and(|stop_reason| {
                            stop_reason == "end_turn" && !assistant_text.is_empty()
                        })
                    {
                        let _ = tx
                            .send(LlmEvent::ResponseComplete(assistant_text.clone()))
                            .await;
                    }
                }
            }
        }
    }

    if !assistant_text.is_empty() {
        let _ = tx
            .send(LlmEvent::ResponseComplete(assistant_text.clone()))
            .await;
    }
    let _ = tx.send(LlmEvent::StreamComplete).await;
    Ok(())
}

async fn process_google_stream(
    response: reqwest::Response,
    tx: mpsc::Sender<LlmEvent>,
) -> std::result::Result<(), LlmError> {
    let mut stream = response.bytes_stream();
    let mut buffer = Vec::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        trace!(event = "llm_stream_chunk", bytes = chunk.len());
        buffer.extend_from_slice(&chunk);
    }

    let buffer_str = String::from_utf8_lossy(&buffer);
    let mut latest_text = String::new();

    if let Ok(response_array) = serde_json::from_str::<Vec<Value>>(&buffer_str) {
        for response_json in response_array {
            if let Some(chunk_text) = extract_google_text(&response_json)
                && chunk_text.len() > latest_text.len()
            {
                latest_text = chunk_text;
            }
        }
    } else if let Ok(response_json) = serde_json::from_str::<Value>(&buffer_str) {
        if let Some(chunk_text) = extract_google_text(&response_json) {
            latest_text = chunk_text;
        }
    } else {
        let mut deserializer = serde_json::Deserializer::from_str(&buffer_str).into_iter::<Value>();
        while let Some(Ok(value)) = deserializer.next() {
            if let Some(chunk_text) = extract_google_text(&value)
                && chunk_text.len() > latest_text.len()
            {
                latest_text = chunk_text;
            }
        }
    }

    if latest_text.is_empty() {
        return Err(LlmError::UnexpectedFormat(
            "Google response did not include text content".to_string(),
        ));
    }

    simulate_streaming(&latest_text, tx.clone()).await;
    let _ = tx
        .send(LlmEvent::ResponseComplete(latest_text.clone()))
        .await;
    let _ = tx.send(LlmEvent::StreamComplete).await;
    Ok(())
}

async fn simulate_streaming(text: &str, tx: mpsc::Sender<LlmEvent>) {
    if text.len() < 50 {
        for ch in text.chars() {
            let _ = tx.send(LlmEvent::TextDelta(ch.to_string())).await;
            sleep(Duration::from_millis(30)).await;
        }
    } else {
        let words: Vec<&str> = text.split_whitespace().collect();

        for (i, word) in words.iter().enumerate() {
            let chunk = if i == 0 {
                word.to_string()
            } else {
                format!(" {word}")
            };

            let _ = tx.send(LlmEvent::TextDelta(chunk)).await;
            sleep(Duration::from_millis(80)).await;
        }
    }
}

fn extract_google_text(value: &Value) -> Option<String> {
    let mut collected = String::new();

    if let Some(candidates) = value.get("candidates").and_then(|c| c.as_array()) {
        for candidate in candidates {
            let parts_iter = candidate
                .get("content")
                .and_then(|content| {
                    if content.is_array() {
                        content.as_array().cloned()
                    } else {
                        content
                            .get("parts")
                            .and_then(|parts| parts.as_array().cloned())
                    }
                })
                .into_iter()
                .flatten();

            for part in parts_iter {
                if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                    collected.push_str(text);
                }
            }
        }
    }

    if collected.is_empty() {
        None
    } else {
        Some(collected)
    }
}

impl LlmRequest {
    /// Construct a new request targeting the specified mode.
    pub fn new(messages: Vec<LlmMessage>, mode: BindrMode) -> Self {
        Self {
            messages,
            mode,
            temperature: None,
            max_tokens: None,
            provider_id: None,
            model_id: None,
        }
    }

    /// Attach a temperature override to the request.
    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    /// Attach a max token override to the request.
    pub fn with_max_tokens(mut self, tokens: u32) -> Self {
        self.max_tokens = Some(tokens);
        self
    }

    /// Specify a provider identifier to override the configuration default.
    pub fn with_provider<S: Into<String>>(mut self, provider: S) -> Self {
        let value = provider.into();
        if !value.is_empty() {
            self.provider_id = Some(value);
        }
        self
    }

    /// Specify a model identifier to override the configuration default.
    pub fn with_model<S: Into<String>>(mut self, model: S) -> Self {
        let value = model.into();
        if !value.is_empty() {
            self.model_id = Some(value);
        }
        self
    }
}
