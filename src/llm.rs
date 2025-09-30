use crate::config::{Config, ModelProvider};
use crate::events::BindrMode;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::time::Duration;
use futures::StreamExt;

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
}

/// Message in conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: String,
    pub content: String,
}

/// LLM client for streaming responses
#[derive(Clone)]
pub struct LlmClient {
    config: Config,
    client: reqwest::Client,
}

impl LlmClient {
    pub fn new(config: Config) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("Failed to create HTTP client");

        Self { config, client }
    }


    /// Stream a response from the configured LLM provider
    pub async fn stream_response(
        &self,
        request: LlmRequest,
    ) -> Result<mpsc::Receiver<LlmEvent>> {
        let (tx, rx) = mpsc::channel(1000);

        // Check if we have an API key configured
        if !self.config.has_api_key() {
            let _ = tx.send(LlmEvent::Error("No API key configured. Please add an API key first.".to_string())).await;
            return Ok(rx);
        }
        
        let provider = self.config.get_current_provider()
            .ok_or_else(|| anyhow::anyhow!("No provider configured"))?;
        
        let api_key = self.config.get_api_key()
            .ok_or_else(|| anyhow::anyhow!("No API key configured"))?;

        // Spawn streaming task
        let client = self.client.clone();
        let provider = provider.clone();
        let model = self.config.default_model.clone();
        
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            if let Err(e) = Self::stream_from_provider(
                client,
                provider,
                model,
                api_key,
                request,
                tx,
            ).await {
                let _ = tx_clone.send(LlmEvent::Error(e.to_string())).await;
            }
        });

        Ok(rx)
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
        match provider.name.as_str() {
            "openai" => Self::stream_openai(client, provider, model, api_key, request, tx).await,
            "anthropic" => Self::stream_anthropic(client, provider, model, api_key, request, tx).await,
            "google" => Self::stream_google(client, provider, model, api_key, request, tx).await,
            "xai" => Self::stream_xai(client, provider, model, api_key, request, tx).await,
            "openrouter" => Self::stream_openrouter(client, provider, model, api_key, request, tx).await,
            "mistral" => Self::stream_mistral(client, provider, model, api_key, request, tx).await,
            _ => Err(anyhow::anyhow!("Unsupported provider: {}", provider.name)),
        }
    }

    /// Stream from OpenAI API
    async fn stream_openai(
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

        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("OpenAI API error: {}", error_text));
        }

        Self::process_sse_stream(response, tx).await
    }

    /// Stream from Anthropic API
    async fn stream_anthropic(
        client: reqwest::Client,
        provider: ModelProvider,
        model: String,
        api_key: String,
        request: LlmRequest,
        tx: mpsc::Sender<LlmEvent>,
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

        let payload = serde_json::json!({
            "model": model,
            "messages": messages,
            "system": system,
            "stream": true,
            "temperature": request.temperature.unwrap_or(0.7),
            "max_tokens": request.max_tokens.unwrap_or(4000)
        });

        let response = client
            .post(&url)
            .header("x-api-key", api_key)
            .header("Content-Type", "application/json")
            .header("anthropic-version", "2023-06-01")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Anthropic API error: {}", error_text));
        }

        Self::process_anthropic_stream(response, tx).await
    }

    /// Stream from Google Gemini API
    async fn stream_google(
        client: reqwest::Client,
        provider: ModelProvider,
        model: String,
        api_key: String,
        request: LlmRequest,
        tx: mpsc::Sender<LlmEvent>,
    ) -> Result<()> {
        let url = format!("{}/v1beta/models/{}:streamGenerateContent?key={}", 
                         provider.base_url, model, api_key);
        
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

        let mut payload = serde_json::json!({
            "contents": contents,
            "generationConfig": {
                "temperature": request.temperature.unwrap_or(0.7),
                "maxOutputTokens": request.max_tokens.unwrap_or(4000)
            }
        });

        if !system_instruction.is_empty() {
            payload["systemInstruction"] = serde_json::json!({
                "parts": [{"text": system_instruction}]
            });
        }

        let response = client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Google API error: {}", error_text));
        }

        Self::process_google_stream(response, tx).await
    }

    /// Stream from xAI Grok API
    async fn stream_xai(
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

        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("xAI API error: {}", error_text));
        }

        Self::process_sse_stream(response, tx).await
    }

    /// Stream from OpenRouter API
    async fn stream_openrouter(
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

        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", "https://bindr.dev")
            .header("X-Title", "Bindr")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("OpenRouter API error: {}", error_text));
        }

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

        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Mistral API error: {}", error_text));
        }

        Self::process_sse_stream(response, tx).await
    }

    /// Process Server-Sent Events stream (OpenAI, xAI, OpenRouter, Mistral)
    async fn process_sse_stream(
        response: reqwest::Response,
        tx: mpsc::Sender<LlmEvent>,
    ) -> Result<()> {
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let text = String::from_utf8_lossy(&chunk);
            buffer.push_str(&text);

            // Process complete lines
            while let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].trim().to_string();
                buffer = buffer[newline_pos + 1..].to_string();

                if line.starts_with("data: ") {
                    let data = &line[6..];
                    if data == "[DONE]" {
                        let _ = tx.send(LlmEvent::StreamComplete).await;
                        return Ok(());
                    }

                    if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(choices) = chunk.get("choices").and_then(|c| c.get(0)) {
                            if let Some(delta) = choices.get("delta") {
                                if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                                    let _ = tx.send(LlmEvent::TextDelta(content.to_string())).await;
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Process Anthropic streaming format
    async fn process_anthropic_stream(
        response: reqwest::Response,
        tx: mpsc::Sender<LlmEvent>,
    ) -> Result<()> {
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let text = String::from_utf8_lossy(&chunk);
            buffer.push_str(&text);

            // Process complete lines
            while let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].trim().to_string();
                buffer = buffer[newline_pos + 1..].to_string();

                if line.starts_with("data: ") {
                    let data = &line[6..];
                    if data == "[DONE]" {
                        let _ = tx.send(LlmEvent::StreamComplete).await;
                        return Ok(());
                    }

                    if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(content_block) = chunk.get("content_block") {
                            if let Some(text) = content_block.get("text").and_then(|t| t.as_str()) {
                                let _ = tx.send(LlmEvent::TextDelta(text.to_string())).await;
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Process Google Gemini streaming format
    async fn process_google_stream(
        response: reqwest::Response,
        tx: mpsc::Sender<LlmEvent>,
    ) -> Result<()> {
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let text = String::from_utf8_lossy(&chunk);
            buffer.push_str(&text);

            // Process complete lines
            while let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].trim().to_string();
                buffer = buffer[newline_pos + 1..].to_string();

                if line.starts_with("data: ") {
                    let data = &line[6..];
                    if data == "[DONE]" {
                        let _ = tx.send(LlmEvent::StreamComplete).await;
                        return Ok(());
                    }

                    if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(candidates) = chunk.get("candidates").and_then(|c| c.get(0)) {
                            if let Some(content) = candidates.get("content") {
                                if let Some(parts) = content.get("parts") {
                                    if let Some(part) = parts.get(0) {
                                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                            let _ = tx.send(LlmEvent::TextDelta(text.to_string())).await;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
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

}
