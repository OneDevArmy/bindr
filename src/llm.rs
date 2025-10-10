use crate::config::{Config, ModelProvider as ProviderConfig};
use crate::events::BindrMode;
use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::StatusCode;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info, trace, warn};

const MAX_RETRIES: usize = 3;
const RETRY_BACKOFF_BASE_MS: u64 = 500;
use tokio::time::Duration;
use tracing::{debug, error, info, trace};

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

/// Request to send to LLM
#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub messages: Vec<LlmMessage>,
    #[allow(dead_code)]
    pub mode: BindrMode,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
}

/// Message in conversation
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
    #[error("{0}")]
    Other(String),
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

/// LLM client for streaming responses
#[derive(Clone)]
pub struct LlmClient {
    config: Config,
    client: reqwest::Client,
    providers: HashMap<String, Arc<dyn ModelProvider>>,
}

impl LlmClient {
    pub fn new(config: Config) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("Failed to create HTTP client");

        let providers: HashMap<String, Arc<dyn ModelProvider>> = vec![
            Arc::new(OpenAiProvider) as Arc<dyn ModelProvider>,
            Arc::new(AnthropicProvider) as Arc<dyn ModelProvider>,
            Arc::new(GoogleProvider) as Arc<dyn ModelProvider>,
            Arc::new(XaiProvider) as Arc<dyn ModelProvider>,
            Arc::new(OpenRouterProvider) as Arc<dyn ModelProvider>,
        ]
        .into_iter()
        .map(|provider| (provider.id().to_string(), provider))
        .collect();

        Self {
            config,
            client,
            providers,
        }
    }

    /// Stream a response from the configured LLM provider
    pub async fn stream_response(
        &self,
        mut request: LlmRequest,
    ) -> Result<mpsc::Receiver<LlmEvent>> {
        let (tx, rx) = mpsc::channel(1000);

    /// Stream a response from the configured LLM provider
    pub async fn stream_response(&self, request: LlmRequest) -> Result<mpsc::Receiver<LlmEvent>> {
        let (tx, rx) = mpsc::channel(1000);

        // Check if we have an API key configured
        if !self.config.has_api_key() {
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
            .ok_or_else(|| anyhow::anyhow!("No provider configured for id {}", provider_id))?;

        let provider_impl =
            self.providers.get(&provider_key).cloned().ok_or_else(|| {
                anyhow::anyhow!("No runtime support for provider {}", provider_id)
            })?;

        let api_key = self
            .config
            .get_api_key_for(&provider_key)

        let provider = self
            .config
            .model_providers
            .get(&provider_id)
            .ok_or_else(|| anyhow::anyhow!("No provider configured for id {}", provider_id))?
            .clone();

        let api_key = self
            .config
            .get_api_key_for(&provider_id)
            .ok_or_else(|| anyhow::anyhow!("No API key configured for provider {}", provider_id))?;

        let model = request
            .model_id
            .take()
            .clone()
            .unwrap_or_else(|| self.config.default_model.clone());

        let message_count = request.messages.len();
        let temperature = request.temperature.unwrap_or(0.7);
        let max_tokens = request.max_tokens.unwrap_or(4000);
        let mode = request.mode;
        let provider_name = provider_config.name.clone();
        let provider_name = provider.name.clone();

        info!(
            event = "llm_api_call",
            provider = provider_name.as_str(),
            provider_id = provider_key.as_str(),
            provider_id = provider_id.as_str(),
            model = model.as_str(),
            mode = mode.display_name(),
            message_count,
            temperature,
            max_tokens,
            "dispatching streaming request"
        );

        // Spawn streaming task
        let client = self.client.clone();

        let tx_clone = tx.clone();

        tokio::spawn(async move {
            if let Err(err) = dispatch_with_retry(provider_for_task, ctx, tx_clone).await {
                error!(event = "llm_stream_error", error = %err);
            if let Err(e) =
                Self::stream_from_provider(client, provider, model, api_key, request, tx).await
            {
                error!(event = "llm_stream_error", error = %e);
                let _ = tx_clone.send(LlmEvent::Error(e.to_string())).await;
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
            Ok(()) => {
                return Ok(());
            }
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
                let _ = tx.send(LlmEvent::Error(err.to_string())).await;
                let _ = tx.send(LlmEvent::StreamComplete).await;
                return Err(err);
            }
    /// Stream from specific provider
    async fn stream_from_provider(
        client: reqwest::Client,
        provider: ModelProvider,
        model: String,
        api_key: String,
        request: LlmRequest,
        tx: mpsc::Sender<LlmEvent>,
    ) -> Result<()> {
        match provider.name.to_lowercase().as_str() {
            "openai" => Self::stream_openai(client, provider, model, api_key, request, tx).await,
            "anthropic" => {
                Self::stream_anthropic(client, provider, model, api_key, request, tx).await
            }
            "google" => Self::stream_google(client, provider, model, api_key, request, tx).await,
            "xai" => Self::stream_xai(client, provider, model, api_key, request, tx).await,
            "openrouter" => {
                Self::stream_openrouter(client, provider, model, api_key, request, tx).await
            }
            "mistral" => Self::stream_mistral(client, provider, model, api_key, request, tx).await,
            _ => Err(anyhow::anyhow!("Unsupported provider: {}", provider.name)),
        }
    }
}

fn retry_delay(attempt: usize) -> Duration {
    let exponent = attempt.saturating_sub(1) as u32;
    let factor = 1u64.saturating_shl(exponent);
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
    ) -> Result<()> {
        let url = format!("{}/v1/chat/completions", provider.base_url);

        let payload = serde_json::json!({
            "model": model,
            "messages": request.messages,
            "stream": true,
            "temperature": temperature,
            "max_tokens": max_tokens
        });

        info!(
            event = "llm_http_request",
            provider = self.id(),
            provider = provider.name.as_str(),
            model = model.as_str(),
            url = %url,
            "sending OpenAI streaming request"
        );

        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        let response = ensure_success(response, self.id()).await?;

        debug!(
            event = "llm_http_response",
            provider = self.id(),
            model = model.as_str(),
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
        if !response.status().is_success() {
            error!(
                event = "llm_http_error",
                provider = provider.name.as_str(),
                model = model.as_str(),
                status = %response.status(),
                "OpenAI API returned error"
            );
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("OpenAI API error: {}", error_text));
        }

        debug!(
            event = "llm_http_response",
            provider = provider.name.as_str(),
            model = model.as_str(),
            status = %response.status(),
            "OpenAI response accepted"
        );

        Self::process_sse_stream(response, tx).await
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
    ) -> Result<()> {
        let url = format!("{}/v1/messages", provider.base_url);

        // Convert messages to Anthropic format
        let mut messages = Vec::new();
        let mut system = String::new();

        for msg in request.messages {
            if msg.role == "system" {
                system = msg.content;
            } else {
                messages.push(serde_json::json!({
                    "role": msg.role,
                    "content": msg.content
                }));
            }
        }

        let temperature = request.temperature.unwrap_or(0.7);
        let max_tokens = request.max_tokens.unwrap_or(4000);
        let payload = serde_json::json!({
            "model": model,
            "messages": messages,
            "system": system,
            "stream": true,
            "temperature": temperature,
            "max_tokens": max_tokens
        });

        info!(
            event = "llm_http_request",
            provider = self.id(),
            provider = provider.name.as_str(),
            model = model.as_str(),
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
            provider = self.id(),
            model = model.as_str(),
            status = %response.status(),
            "Anthropic response accepted"
        );

        process_anthropic_stream(response, tx).await
        if !response.status().is_success() {
            error!(
                event = "llm_http_error",
                provider = provider.name.as_str(),
                model = model.as_str(),
                status = %response.status(),
                "Anthropic API returned error"
            );
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Anthropic API error: {}", error_text));
        }

        debug!(
            event = "llm_http_response",
            provider = provider.name.as_str(),
            model = model.as_str(),
            status = %response.status(),
            "Anthropic response accepted"
        );

        Self::process_anthropic_stream(response, tx).await
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
        let url = format!(
            "{}/models/{}:streamGenerateContent?key={}",
            base, model, api_key
        );

    ) -> Result<()> {
        let url = format!(
            "{}/models/{}:streamGenerateContent?key={}",
            provider.base_url, model, api_key
        );

        // Convert messages to Gemini format
        let mut contents = Vec::new();
        let mut system_instruction = String::new();

        for msg in request.messages {
            if msg.role == "system" {
                system_instruction = msg.content;
            } else {
                contents.push(serde_json::json!({
                    "role": msg.role,
                    "parts": [{"text": msg.content}]
                }));
            }
        }

        let temperature = request.temperature.unwrap_or(0.7);
        let max_tokens = request.max_tokens.unwrap_or(4000);
        let mut payload = serde_json::json!({
            "contents": contents,
            "generationConfig": {
                "temperature": temperature,
                "maxOutputTokens": max_tokens
            }
        });

        if !system_instruction.is_empty() {
            payload["systemInstruction"] = serde_json::json!({
                "parts": [{"text": system_instruction}]
            });
        }

        info!(
            event = "llm_http_request",
            provider = self.id(),
            provider = provider.name.as_str(),
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
            provider = self.id(),
            model = model.as_str(),
            status = %response.status(),
            "Google Gemini response accepted"
        );

        process_google_stream(response, tx).await
        if !response.status().is_success() {
            error!(
                event = "llm_http_error",
                provider = provider.name.as_str(),
                model = model.as_str(),
                status = %response.status(),
                "Google Gemini API returned error"
            );
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Google API error: {}", error_text));
        }

        debug!(
            event = "llm_http_response",
            provider = provider.name.as_str(),
            model = model.as_str(),
            status = %response.status(),
            "Google Gemini response accepted"
        );

        Self::process_google_stream(response, tx).await
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
    ) -> Result<()> {
        let url = format!("{}/v1/chat/completions", provider.base_url);

        let payload = serde_json::json!({
            "model": model,
            "messages": request.messages,
            "stream": true,
            "temperature": temperature,
            "max_tokens": max_tokens
        });

        info!(
            event = "llm_http_request",
            provider = self.id(),
            provider = provider.name.as_str(),
            model = model.as_str(),
            url = %url,
            "sending xAI streaming request"
        );

        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        let response = ensure_success(response, self.id()).await?;

        debug!(
            event = "llm_http_response",
            provider = self.id(),
            model = model.as_str(),
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
        if !response.status().is_success() {
            error!(
                event = "llm_http_error",
                provider = provider.name.as_str(),
                model = model.as_str(),
                status = %response.status(),
                "xAI API returned error"
            );
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("xAI API error: {}", error_text));
        }

        debug!(
            event = "llm_http_response",
            provider = provider.name.as_str(),
            model = model.as_str(),
            status = %response.status(),
            "xAI response accepted"
        );

        Self::process_sse_stream(response, tx).await
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
    ) -> Result<()> {
        let url = format!("{}/v1/chat/completions", provider.base_url);

        let payload = serde_json::json!({
            "model": model,
            "messages": request.messages,
            "stream": true,
            "temperature": temperature,
            "max_tokens": max_tokens
        });

        info!(
            event = "llm_http_request",
            provider = self.id(),
            provider = provider.name.as_str(),
            model = model.as_str(),
            url = %url,
            "sending OpenRouter streaming request"
        );

        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", "https://bindr.dev")
            .header("X-Title", "Bindr")
            .json(&payload)
            .send()
            .await?;

        let response = ensure_success(response, self.id()).await?;

        debug!(
            event = "llm_http_response",
            provider = self.id(),
            model = model.as_str(),
            status = %response.status(),
            "OpenRouter response accepted"
        );

        process_sse_stream(response, tx).await
    }
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
        if !response.status().is_success() {
            error!(
                event = "llm_http_error",
                provider = provider.name.as_str(),
                model = model.as_str(),
                status = %response.status(),
                "OpenRouter API returned error"
            );
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            let detail = if error_text.trim().is_empty() {
                "<empty response>".to_string()
            } else {
                error_text
            };
            return Err(anyhow::anyhow!(
                "OpenRouter API error (status {}): {}",
                status,
                detail
            ));
        }

        debug!(
            event = "llm_http_response",
            provider = provider.name.as_str(),
            model = model.as_str(),
            status = %response.status(),
            "OpenRouter response accepted"
        );

        Self::process_sse_stream(response, tx).await
    }

    /// Stream from Mistral AI API
    async fn stream_mistral(
        client: reqwest::Client,
        provider: ModelProvider,
        model: String,
        api_key: String,
        request: LlmRequest,
        tx: mpsc::Sender<LlmEvent>,
    ) -> Result<()> {
        let url = format!("{}/v1/chat/completions", provider.base_url);

        let payload = serde_json::json!({
            "model": model,
            "messages": request.messages,
            "stream": true,
            "temperature": request.temperature.unwrap_or(0.7),
            "max_tokens": request.max_tokens.unwrap_or(4000)
        });

        info!(
            event = "llm_http_request",
            provider = provider.name.as_str(),
            model = model.as_str(),
            url = %url,
            "sending Mistral streaming request"
        );

        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            error!(
                event = "llm_http_error",
                provider = provider.name.as_str(),
                model = model.as_str(),
                status = %response.status(),
                "Mistral API returned error"
            );
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Mistral API error: {}", error_text));
        }

        debug!(
            event = "llm_http_response",
            provider = provider.name.as_str(),
            model = model.as_str(),
            status = %response.status(),
            "Mistral response accepted"
        );

        Self::process_sse_stream(response, tx).await
    }
}

fn join_url(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    format!("{base}/{path}")
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

            if line.starts_with("data: ") {
                let data = &line[6..];
                if data == "[DONE]" {
                    if !assistant_text.is_empty() {
                        let _ = tx.send(LlmEvent::ResponseComplete(assistant_text)).await;
    /// Process Server-Sent Events stream (OpenAI, xAI, OpenRouter, Mistral)
    async fn process_sse_stream(
        response: reqwest::Response,
        tx: mpsc::Sender<LlmEvent>,
    ) -> Result<()> {
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut assistant_text = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            trace!(event = "llm_stream_chunk", bytes = chunk.len());
            let text = String::from_utf8_lossy(&chunk);
            buffer.push_str(&text);

            // Process complete lines
            while let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].trim().to_string();
                buffer = buffer[newline_pos + 1..].to_string();

                if line.starts_with("data: ") {
                    let data = &line[6..];
                    if data == "[DONE]" {
                        // Emit final accumulated message if we have content
                        if !assistant_text.is_empty() {
                            let _ = tx.send(LlmEvent::ResponseComplete(assistant_text)).await;
                        }
                        let _ = tx.send(LlmEvent::StreamComplete).await;
                        return Ok(());
                    }

                    if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(choices) = chunk.get("choices").and_then(|c| c.get(0)) {
                            // Handle streaming deltas
                            if let Some(delta) = choices.get("delta") {
                                if let Some(content) = delta.get("content").and_then(|c| c.as_str())
                                {
                                    assistant_text.push_str(content);
                                    let _ = tx.send(LlmEvent::TextDelta(content.to_string())).await;
                                }
                            }

                            // Handle finish_reason
                            if let Some(finish_reason) =
                                choices.get("finish_reason").and_then(|v| v.as_str())
                            {
                                if finish_reason == "stop" && !assistant_text.is_empty() {
                                    let _ = tx
                                        .send(LlmEvent::ResponseComplete(assistant_text.clone()))
                                        .await;
                                }
                            }
                        }
                    }
                    let _ = tx.send(LlmEvent::StreamComplete).await;
                    return Ok(());
                }

                if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(choice) = chunk.get("choices").and_then(|c| c.get(0)) {
                        if let Some(delta) = choice.get("delta") {
                            if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                                assistant_text.push_str(content);
                                let _ = tx.send(LlmEvent::TextDelta(content.to_string())).await;
                            }

                            if let Some(reasoning) = delta.get("reasoning").and_then(|r| r.as_str())
                            {
                                let _ = tx
                                    .send(LlmEvent::ReasoningDelta(reasoning.to_string()))
                                    .await;
                            }
                        }

                        if let Some(finish_reason) =
                            choice.get("finish_reason").and_then(|v| v.as_str())
                        {
                            if finish_reason == "stop" && !assistant_text.is_empty() {
                                let _ = tx
                                    .send(LlmEvent::ResponseComplete(assistant_text.clone()))
                                    .await;
                            }
                        }
                    }
                }
            }
        }
    }

    let line = buffer.trim();
    if line.starts_with("data: ") {
        let data = &line[6..];
        if data != "[DONE]" {
            if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(choice) = chunk.get("choices").and_then(|c| c.get(0)) {
                    if let Some(delta) = choice.get("delta") {
                        if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                            assistant_text.push_str(content);
                            let _ = tx.send(LlmEvent::TextDelta(content.to_string())).await;
    /// Process Anthropic streaming format
    async fn process_anthropic_stream(
        response: reqwest::Response,
        tx: mpsc::Sender<LlmEvent>,
    ) -> Result<()> {
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut assistant_text = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            trace!(event = "llm_stream_chunk", bytes = chunk.len());
            let text = String::from_utf8_lossy(&chunk);
            buffer.push_str(&text);

            // Process complete lines
            while let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].trim().to_string();
                buffer = buffer[newline_pos + 1..].to_string();

                if line.starts_with("data: ") {
                    let data = &line[6..];
                    if data == "[DONE]" {
                        // Emit final accumulated message if we have content
                        if !assistant_text.is_empty() {
                            let _ = tx.send(LlmEvent::ResponseComplete(assistant_text)).await;
                        }
                        let _ = tx.send(LlmEvent::StreamComplete).await;
                        return Ok(());
                    }

                    if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(content_block) = chunk.get("content_block") {
                            if let Some(text) = content_block.get("text").and_then(|t| t.as_str()) {
                                assistant_text.push_str(text);
                                let _ = tx.send(LlmEvent::TextDelta(text.to_string())).await;
                            }
                        }

                        // Handle stop event
                        if let Some(stop_reason) = chunk.get("stop_reason").and_then(|v| v.as_str())
                        {
                            if stop_reason == "end_turn" && !assistant_text.is_empty() {
                                let _ = tx
                                    .send(LlmEvent::ResponseComplete(assistant_text.clone()))
                                    .await;
                            }
                        }
                    }
                }
            }
        }
    }

    if !assistant_text.is_empty() {
        let _ = tx.send(LlmEvent::ResponseComplete(assistant_text)).await;
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

            if line.starts_with("data: ") {
                let data = &line[6..];
                if data == "[DONE]" {
                    if !assistant_text.is_empty() {
                        let _ = tx.send(LlmEvent::ResponseComplete(assistant_text)).await;
                    }
                    let _ = tx.send(LlmEvent::StreamComplete).await;
                    return Ok(());
                }

                if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(content_block) = chunk.get("content_block") {
                        if let Some(text) = content_block.get("text").and_then(|t| t.as_str()) {
                            assistant_text.push_str(text);
                            let _ = tx.send(LlmEvent::TextDelta(text.to_string())).await;
                        }
                    }

                    if let Some(stop_reason) = chunk.get("stop_reason").and_then(|v| v.as_str()) {
                        if stop_reason == "end_turn" && !assistant_text.is_empty() {
                            let _ = tx
                                .send(LlmEvent::ResponseComplete(assistant_text.clone()))
                                .await;
                        }
                    }
                }
            }
        }
    }

    let line = buffer.trim();
    if line.starts_with("data: ") {
        let data = &line[6..];
        if data != "[DONE]" {
            if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(content_block) = chunk.get("content_block") {
                    if let Some(text) = content_block.get("text").and_then(|t| t.as_str()) {
                        assistant_text.push_str(text);
                        let _ = tx.send(LlmEvent::TextDelta(text.to_string())).await;
                    }
                }
            }
        }
    }

    if !assistant_text.is_empty() {
        let _ = tx.send(LlmEvent::ResponseComplete(assistant_text)).await;
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
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            trace!(event = "llm_stream_chunk", bytes = chunk.len());
            buffer.extend_from_slice(&chunk);
        }

    let buffer_str = String::from_utf8_lossy(&buffer);
    let mut latest_text = String::new();

    if let Ok(response_array) = serde_json::from_str::<Vec<serde_json::Value>>(&buffer_str) {
        for response_json in response_array {
            if let Some(chunk_text) = extract_google_text(&response_json) {
                if chunk_text.len() > latest_text.len() {
                    latest_text = chunk_text;
                }
            }
        }
    } else if let Ok(response_json) = serde_json::from_str::<serde_json::Value>(&buffer_str) {
        if let Some(chunk_text) = extract_google_text(&response_json) {
            if chunk_text.len() > latest_text.len() {
                latest_text = chunk_text;
            }
        }
    } else {
        let mut deserializer =
            serde_json::Deserializer::from_str(&buffer_str).into_iter::<serde_json::Value>();
        while let Some(Ok(value)) = deserializer.next() {
            if let Some(chunk_text) = extract_google_text(&value) {
                if chunk_text.len() > latest_text.len() {
                    latest_text = chunk_text;
        } else {
            // Attempt to parse concatenated JSON objects without delimiters
            let mut deserializer =
                serde_json::Deserializer::from_str(&buffer_str).into_iter::<serde_json::Value>();
            while let Some(Ok(value)) = deserializer.next() {
                if let Some(chunk_text) = Self::extract_google_text(&value) {
                    if chunk_text.len() > latest_text.len() {
                        latest_text = chunk_text;
                    }
                }
            }
        }
    }

    if !latest_text.is_empty() {
        simulate_streaming(&latest_text, tx.clone()).await;
        let _ = tx.send(LlmEvent::ResponseComplete(latest_text)).await;
    }
    let _ = tx.send(LlmEvent::StreamComplete).await;
    Ok(())
}

async fn simulate_streaming(text: &str, tx: mpsc::Sender<LlmEvent>) {
    if text.len() < 50 {
        for ch in text.chars() {
            let _ = tx.send(LlmEvent::TextDelta(ch.to_string())).await;
            sleep(Duration::from_millis(30)).await;
    /// Simulate streaming by breaking text into chunks with delays
    async fn simulate_streaming(text: &str, tx: mpsc::Sender<LlmEvent>) {
        // For short responses, stream character by character
        // For longer responses, stream word by word
        if text.len() < 50 {
            for ch in text.chars() {
                let _ = tx.send(LlmEvent::TextDelta(ch.to_string())).await;
                tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;
            }
        } else {
            let words: Vec<&str> = text.split_whitespace().collect();

            for (i, word) in words.iter().enumerate() {
                let chunk = if i == 0 {
                    word.to_string()
                } else {
                    format!(" {}", word)
                };

                // Send the chunk
                let _ = tx.send(LlmEvent::TextDelta(chunk)).await;

                // Add a small delay to simulate typing
                tokio::time::sleep(tokio::time::Duration::from_millis(80)).await;
            }
        }
    } else {
        let words: Vec<&str> = text.split_whitespace().collect();

        for (i, word) in words.iter().enumerate() {
            let chunk = if i == 0 {
                word.to_string()
            } else {
                format!(" {}", word)
            };

            let _ = tx.send(LlmEvent::TextDelta(chunk)).await;
            sleep(Duration::from_millis(80)).await;
        }
    }
}
    /// Extract concatenated text from Google response JSON
    fn extract_google_text(value: &serde_json::Value) -> Option<String> {
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

fn extract_google_text(value: &serde_json::Value) -> Option<String> {
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

/// Helper to create system messages for different modes
impl LlmRequest {
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

    #[allow(dead_code)]
    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    #[allow(dead_code)]
    pub fn with_max_tokens(mut self, tokens: u32) -> Self {
        self.max_tokens = Some(tokens);
        self
    }

    pub fn with_provider<S: Into<String>>(mut self, provider: S) -> Self {
        let value = provider.into();
        if !value.is_empty() {
            self.provider_id = Some(value);
        }
        self
    }

    pub fn with_model<S: Into<String>>(mut self, model: S) -> Self {
        let value = model.into();
        if !value.is_empty() {
            self.model_id = Some(value);
        }
        self
    }
}
