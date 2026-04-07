use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

use super::config::IntelligenceConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("API error: {0}")]
    Api(String),
    #[error("No API key configured")]
    NoApiKey,
    #[error("Provider not configured")]
    NotConfigured,
    #[error("Embedding unavailable: {0}")]
    EmbeddingUnavailable(String),
}

#[async_trait]
pub trait IntelligenceProvider: Send + Sync {
    async fn complete(&self, messages: Vec<Message>) -> Result<String, ProviderError>;
    async fn embed(&self, text: &str) -> Result<Vec<f32>, ProviderError>;
}

// ── Embedding endpoint enum ────────────────────────────────────────────────

enum EmbeddingEndpoint {
    Ollama { base_url: String, model: String },
    OpenAi { base_url: String, model: String, api_key: String },
}

// ── ApiProvider ────────────────────────────────────────────────────────────

pub struct ApiProvider {
    client: Client,
    api_key: String,
    model: String,
    embedding: Option<EmbeddingEndpoint>,
}

impl ApiProvider {
    pub fn new(config: &IntelligenceConfig) -> Result<Self, ProviderError> {
        let api_key = config.resolve_api_key().ok_or(ProviderError::NoApiKey)?;
        let model = config
            .model
            .clone()
            .unwrap_or_else(|| "claude-3-5-sonnet-20241022".to_string());

        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| ProviderError::Http(e.to_string()))?;

        let embedding = build_embedding_endpoint(config);

        Ok(Self { client, api_key, model, embedding })
    }
}

#[async_trait]
impl IntelligenceProvider for ApiProvider {
    async fn complete(&self, messages: Vec<Message>) -> Result<String, ProviderError> {
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 4096,
            "messages": messages,
        });

        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api(format!("{}: {}", status, text)));
        }

        let json: serde_json::Value =
            resp.json().await.map_err(|e| ProviderError::Http(e.to_string()))?;

        let text = json["content"][0]["text"]
            .as_str()
            .ok_or_else(|| ProviderError::Api("Unexpected response shape".to_string()))?
            .to_string();

        Ok(text)
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>, ProviderError> {
        match &self.embedding {
            Some(EmbeddingEndpoint::Ollama { base_url, model }) => {
                embed_ollama(&self.client, base_url, model, text).await
            }
            Some(EmbeddingEndpoint::OpenAi { base_url, model, api_key }) => {
                embed_openai(&self.client, base_url, model, api_key, text).await
            }
            None => Err(ProviderError::EmbeddingUnavailable(
                "No embedding provider configured".to_string(),
            )),
        }
    }
}

// ── OllamaProvider ─────────────────────────────────────────────────────────

pub struct OllamaProvider {
    client: Client,
    base_url: String,
    model: String,
    embedding_model: Option<String>,
}

impl OllamaProvider {
    pub fn new(config: &IntelligenceConfig) -> Result<Self, ProviderError> {
        let model = config
            .model
            .clone()
            .unwrap_or_else(|| "llama3".to_string());
        let embedding_model = config.embedding_model.clone();

        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| ProviderError::Http(e.to_string()))?;

        Ok(Self {
            client,
            base_url: "http://localhost:11434".to_string(),
            model,
            embedding_model,
        })
    }
}

#[async_trait]
impl IntelligenceProvider for OllamaProvider {
    async fn complete(&self, messages: Vec<Message>) -> Result<String, ProviderError> {
        let body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": false,
        });

        let url = format!("{}/api/chat", self.base_url);
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api(format!("{}: {}", status, text)));
        }

        let json: serde_json::Value =
            resp.json().await.map_err(|e| ProviderError::Http(e.to_string()))?;

        let text = json["message"]["content"]
            .as_str()
            .ok_or_else(|| ProviderError::Api("Unexpected response shape".to_string()))?
            .to_string();

        Ok(text)
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>, ProviderError> {
        let model = self.embedding_model.as_deref().unwrap_or(&self.model);
        embed_ollama(&self.client, &self.base_url, model, text).await
    }
}

// ── Helper functions ───────────────────────────────────────────────────────

pub async fn embed_ollama(
    client: &Client,
    base_url: &str,
    model: &str,
    text: &str,
) -> Result<Vec<f32>, ProviderError> {
    let url = format!("{}/api/embed", base_url);
    let body = serde_json::json!({ "model": model, "input": text });

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| ProviderError::Http(e.to_string()))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(ProviderError::EmbeddingUnavailable(format!("{}: {}", status, text)));
    }

    let json: serde_json::Value =
        resp.json().await.map_err(|e| ProviderError::Http(e.to_string()))?;

    let embeddings = json["embeddings"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            ProviderError::EmbeddingUnavailable("Missing embeddings[0] in response".to_string())
        })?;

    let vec = embeddings
        .iter()
        .map(|v| v.as_f64().unwrap_or(0.0) as f32)
        .collect();

    Ok(vec)
}

pub async fn embed_openai(
    client: &Client,
    base_url: &str,
    model: &str,
    api_key: &str,
    text: &str,
) -> Result<Vec<f32>, ProviderError> {
    let url = format!("{}/v1/embeddings", base_url);
    let body = serde_json::json!({ "model": model, "input": text });

    let resp = client
        .post(&url)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| ProviderError::Http(e.to_string()))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(ProviderError::EmbeddingUnavailable(format!("{}: {}", status, text)));
    }

    let json: serde_json::Value =
        resp.json().await.map_err(|e| ProviderError::Http(e.to_string()))?;

    let embedding = json["data"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|v| v["embedding"].as_array())
        .ok_or_else(|| {
            ProviderError::EmbeddingUnavailable(
                "Missing data[0].embedding in response".to_string(),
            )
        })?;

    let vec = embedding
        .iter()
        .map(|v| v.as_f64().unwrap_or(0.0) as f32)
        .collect();

    Ok(vec)
}

// ── Factory ────────────────────────────────────────────────────────────────

fn build_embedding_endpoint(config: &IntelligenceConfig) -> Option<EmbeddingEndpoint> {
    let ep_provider = config.embedding_provider.as_deref()?;
    let model = config.embedding_model.clone()?;

    match ep_provider {
        "ollama" => Some(EmbeddingEndpoint::Ollama {
            base_url: "http://localhost:11434".to_string(),
            model,
        }),
        "openai" => {
            let api_key = std::env::var("OPENAI_API_KEY").ok().filter(|k| !k.is_empty())?;
            Some(EmbeddingEndpoint::OpenAi {
                base_url: "https://api.openai.com".to_string(),
                model,
                api_key,
            })
        }
        _ => None,
    }
}

pub fn create_provider(
    config: &IntelligenceConfig,
) -> Option<Box<dyn IntelligenceProvider>> {
    if !config.enabled {
        return None;
    }

    match config.provider.as_deref()? {
        "api" => ApiProvider::new(config).ok().map(|p| Box::new(p) as Box<dyn IntelligenceProvider>),
        "ollama" => OllamaProvider::new(config).ok().map(|p| Box::new(p) as Box<dyn IntelligenceProvider>),
        _ => None,
    }
}
