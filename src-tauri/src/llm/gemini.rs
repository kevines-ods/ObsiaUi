use crate::llm::provider::{ChatRequest, ChatResponse, LlmError, LlmProvider, ModelInfo, ModelCapability, ModelPricing, TokenEvent, TokenStream};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, instrument};

#[derive(Serialize, Deserialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize, Deserialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    generation_config: GeminiGenerationConfig,
}

#[derive(Serialize)]
struct GeminiGenerationConfig {
    temperature: Option<f32>,
    max_output_tokens: Option<u32>,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    usage_metadata: Option<GeminiUsage>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: GeminiContent,
    finish_reason: Option<String>,
    index: u32,
}

#[derive(Deserialize)]
struct GeminiUsage {
    prompt_token_count: u32,
    candidates_token_count: u32,
    total_token_count: u32,
}

#[derive(Deserialize)]
struct GeminiStreamResponse {
    candidates: Vec<GeminiStreamCandidate>,
}

#[derive(Deserialize)]
struct GeminiStreamCandidate {
    content: GeminiStreamContent,
    finish_reason: Option<String>,
    index: u32,
}

#[derive(Deserialize)]
struct GeminiStreamContent {
    parts: Vec<GeminiPart>,
    role: String,
}

pub struct GeminiProvider {
    client: Arc<Client>,
    api_key: String,
    base_url: String,
}

impl GeminiProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        let key = api_key.into();
        let client = Arc::new(Client::new());
        Self { client, api_key: key, base_url: "https://generativelanguage.googleapis.com/v1beta".to_string() }
    }

    pub fn from_env() -> Result<Self, LlmError> {
        let key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| LlmError::AuthFailed("GEMINI_API_KEY not set".into()))?;
        Ok(Self::new(key))
    }
}

#[async_trait]
impl LlmProvider for GeminiProvider {
    fn id(&self) -> &str { "gemini" }
    fn name(&self) -> &str { "Google Gemini" }

    #[instrument(skip(self))]
    async fn health_check(&self) -> Result<(), LlmError> {
        let req = GeminiRequest {
            contents: vec![GeminiContent {
                role: "user".to_string(),
                parts: vec![GeminiPart { text: "ping".to_string() }],
            }],
            generation_config: GeminiGenerationConfig { temperature: None, max_output_tokens: Some(1) },
        };
        let resp = self.client.post(format!("{}/models/gemini-1.5-flash:generateContent?key={}", self.base_url, self.api_key))
            .json(&req)
            .send()
            .await
            .map_err(|e| LlmError::ProviderUnavailable(e.to_string()))?;
        if resp.status().is_success() {
            info!("Gemini health check OK");
            Ok(())
        } else {
            Err(LlmError::ProviderUnavailable("Gemini not responding".into()))
        }
    }

    #[instrument(skip(self))]
    async fn list_models(&self) -> Result<Vec<ModelInfo>, LlmError> {
        Ok(vec![
            ModelInfo { id: "gemini-1.5-pro".into(), name: "Gemini 1.5 Pro".into(), provider: "gemini".into(), context_window: 2_000_000, capabilities: vec![ModelCapability::Chat, ModelCapability::Vision, ModelCapability::Audio, ModelCapability::ToolUse, ModelCapability::Reasoning], pricing: Some(ModelPricing { input_per_1k: 0.00125, output_per_1k: 0.005, currency: "USD".into() }), local_path: None },
            ModelInfo { id: "gemini-1.5-flash".into(), name: "Gemini 1.5 Flash".into(), provider: "gemini".into(), context_window: 1_000_000, capabilities: vec![ModelCapability::Chat, ModelCapability::Vision, ModelCapability::Audio, ModelCapability::ToolUse], pricing: Some(ModelPricing { input_per_1k: 0.000075, output_per_1k: 0.0003, currency: "USD".into() }), local_path: None },
            ModelInfo { id: "gemini-1.0-pro".into(), name: "Gemini 1.0 Pro".into(), provider: "gemini".into(), context_window: 32_768, capabilities: vec![ModelCapability::Chat, ModelCapability::ToolUse], pricing: Some(ModelPricing { input_per_1k: 0.0005, output_per_1k: 0.0015, currency: "USD".into() }), local_path: None },
        ])
    }

    #[instrument(skip(self, req))]
    async fn chat_stream(&self, req: ChatRequest) -> Result<TokenStream, LlmError> {
        let (tx, rx) = mpsc::channel(100);
        let client = self.client.clone();
        let api_key = self.api_key.clone();
        let base_url = self.base_url.clone();
        let model = req.model.clone();
        let contents: Vec<GeminiContent> = req.messages.into_iter().map(|m| GeminiContent {
            role: if m.role == "assistant" { "model" } else { "user" }.to_string(),
            parts: vec![GeminiPart { text: m.content }],
        }).collect();

        tokio::spawn(async move {
            let request = GeminiRequest {
                contents,
                generation_config: GeminiGenerationConfig {
                    temperature: req.temperature,
                    max_output_tokens: req.max_tokens,
                },
            };

            let resp = client.post(format!("{}/models/{}:streamGenerateContent?key={}", base_url, model, api_key))
                .json(&request)
                .send()
                .await;

            let mut resp = match resp {
                Ok(r) => r,
                Err(e) => { let _ = tx.send(TokenEvent::Error(e.to_string())).await; return; }
            };

            let mut full_content = String::new();
            use futures::StreamExt;
            let mut stream = resp.bytes_stream();
            let mut buffer = String::new();
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));
                        for line in buffer.lines() {
                            if let Ok(stream_resp) = serde_json::from_str::<GeminiStreamResponse>(line) {
                                if let Some(candidate) = stream_resp.candidates.first() {
                                    if let Some(part) = candidate.content.parts.first() {
                                        full_content.push_str(&part.text);
                                        let _ = tx.send(TokenEvent::Token(part.text.clone())).await;
                                    }
                                    if candidate.finish_reason.is_some() {
                                        let _ = tx.send(TokenEvent::Done(ChatResponse {
                                            id: uuid::Uuid::new_v4().to_string(),
                                            model: model.clone(),
                                            choices: vec![crate::llm::provider::ChatChoice {
                                                index: 0,
                                                message: crate::llm::provider::ChatMessage { role: "assistant".into(), content: full_content.clone(), name: None },
                                                finish_reason: candidate.finish_reason.clone(),
                                            }],
                                            usage: None,
                                        })).await;
                                        return;
                                    }
                                }
                            }
                        }
                        buffer.clear();
                    }
                    Err(e) => { let _ = tx.send(TokenEvent::Error(e.to_string())).await; return; }
                }
            }
        });

        Ok(rx)
    }

    #[instrument(skip(self, req))]
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, LlmError> {
        let contents: Vec<GeminiContent> = req.messages.into_iter().map(|m| GeminiContent {
            role: if m.role == "assistant" { "model" } else { "user" }.to_string(),
            parts: vec![GeminiPart { text: m.content }],
        }).collect();

        let request = GeminiRequest {
            contents,
            generation_config: GeminiGenerationConfig {
                temperature: req.temperature,
                max_output_tokens: req.max_tokens,
            },
        };

        let resp = self.client.post(format!("{}/models/{}:generateContent?key={}", self.base_url, req.model, self.api_key))
            .json(&request)
            .send()
            .await
            .map_err(|e| LlmError::ApiError(e.to_string()))?;

        let data: GeminiResponse = resp.json().await
            .map_err(|e| LlmError::ApiError(e.to_string()))?;

        let candidate = data.candidates.first().ok_or(LlmError::Internal("No candidates".into()))?;
        let text = candidate.content.parts.first().map(|p| p.text.clone()).unwrap_or_default();

        Ok(ChatResponse {
            id: uuid::Uuid::new_v4().to_string(),
            model: req.model,
            choices: vec![crate::llm::provider::ChatChoice {
                index: 0,
                message: crate::llm::provider::ChatMessage { role: "assistant".into(), content: text, name: None },
                finish_reason: candidate.finish_reason.clone(),
            }],
            usage: data.usage_metadata.map(|u| crate::llm::provider::Usage {
                prompt_tokens: u.prompt_token_count,
                completion_tokens: u.candidates_token_count,
                total_tokens: u.total_token_count,
            }),
        })
    }
}