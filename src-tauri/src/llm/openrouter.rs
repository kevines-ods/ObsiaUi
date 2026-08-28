use crate::llm::provider::{ChatRequest, ChatResponse, LlmError, LlmProvider, ModelInfo, ModelCapability, ModelPricing, TokenEvent, TokenStream};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, instrument};

#[derive(Serialize, Deserialize)]
struct OpenRouterMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct OpenRouterRequest {
    model: String,
    messages: Vec<OpenRouterMessage>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    stream: bool,
}

#[derive(Deserialize)]
struct OpenRouterResponse {
    id: String,
    model: String,
    choices: Vec<OpenRouterChoice>,
    usage: Option<OpenRouterUsage>,
}

#[derive(Deserialize)]
struct OpenRouterChoice {
    index: u32,
    message: OpenRouterMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenRouterUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Deserialize)]
struct OpenRouterStreamChunk {
    id: String,
    model: String,
    choices: Vec<OpenRouterStreamChoice>,
}

#[derive(Deserialize)]
struct OpenRouterStreamChoice {
    index: u32,
    delta: OpenRouterDelta,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenRouterDelta {
    content: Option<String>,
}

pub struct OpenRouterProvider {
    client: Arc<Client>,
    api_key: String,
    base_url: String,
}

impl OpenRouterProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        let key = api_key.into();
        let client = Arc::new(Client::new());
        Self { client, api_key: key, base_url: "https://openrouter.ai/api/v1".to_string() }
    }

    pub fn from_env() -> Result<Self, LlmError> {
        let key = std::env::var("OPENROUTER_API_KEY")
            .map_err(|_| LlmError::AuthFailed("OPENROUTER_API_KEY not set".into()))?;
        Ok(Self::new(key))
    }
}

#[async_trait]
impl LlmProvider for OpenRouterProvider {
    fn id(&self) -> &str { "openrouter" }
    fn name(&self) -> &str { "OpenRouter" }

    #[instrument(skip(self))]
    async fn health_check(&self) -> Result<(), LlmError> {
        let req = OpenRouterRequest {
            model: "openrouter/auto".to_string(),
            messages: vec![OpenRouterMessage { role: "user".to_string(), content: "ping".to_string() }],
            max_tokens: Some(1),
            temperature: None,
            stream: false,
        };
        let resp = self.client.post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&req)
            .send()
            .await
            .map_err(|e| LlmError::ProviderUnavailable(e.to_string()))?;
        if resp.status().is_success() {
            info!("OpenRouter health check OK");
            Ok(())
        } else {
            Err(LlmError::ProviderUnavailable("OpenRouter not responding".into()))
        }
    }

    #[instrument(skip(self))]
    async fn list_models(&self) -> Result<Vec<ModelInfo>, LlmError> {
        let resp = self.client.get(format!("{}/models", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| LlmError::ApiError(e.to_string()))?;
        let data: serde_json::Value = resp.json().await
            .map_err(|e| LlmError::ApiError(e.to_string()))?;
        let models_array = data["data"].as_array().cloned().unwrap_or_default();
        let models = models_array.as_slice();
        Ok(models.iter().filter_map(|m| {
            m["id"].as_str().map(|id| {
                let input_price = m["pricing"]["prompt"].as_str().and_then(|s| s.parse::<f64>().ok());
                let output_price = m["pricing"]["completion"].as_str().and_then(|s| s.parse::<f64>().ok());
                let pricing = input_price.zip(output_price).map(|(input, output)| ModelPricing {
                    input_per_1k: input,
                    output_per_1k: output,
                    currency: "USD".into(),
                });
                ModelInfo {
                    id: id.to_string(),
                    name: m["name"].as_str().unwrap_or(id).to_string(),
                    provider: "openrouter".to_string(),
                    context_window: m["context_length"].as_u64().unwrap_or(4096) as u32,
                    capabilities: vec![ModelCapability::Chat],
                    pricing,
                    local_path: None,
                }
            })
        }).collect())
    }

    #[instrument(skip(self, req))]
    async fn chat_stream(&self, req: ChatRequest) -> Result<TokenStream, LlmError> {
        let (tx, rx) = mpsc::channel(100);
        let client = self.client.clone();
        let api_key = self.api_key.clone();
        let base_url = self.base_url.clone();
        let model = req.model.clone();
        let messages: Vec<OpenRouterMessage> = req.messages.into_iter().map(|m| OpenRouterMessage {
            role: m.role,
            content: m.content,
        }).collect();

        tokio::spawn(async move {
            let request = OpenRouterRequest {
                model,
                messages,
                max_tokens: req.max_tokens,
                temperature: req.temperature,
                stream: true,
            };

            let resp = client.post(format!("{}/chat/completions", base_url))
                .header("Authorization", format!("Bearer {}", api_key))
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
                            if line.starts_with("data: ") {
                                let data = &line[6..];
                                if data == "[DONE]" {
                                    let _ = tx.send(TokenEvent::Done(ChatResponse {
                                        id: uuid::Uuid::new_v4().to_string(),
                                        model: request.model.clone(),
                                        choices: vec![crate::llm::provider::ChatChoice {
                                            index: 0,
                                            message: crate::llm::provider::ChatMessage { role: "assistant".into(), content: full_content, name: None },
                                            finish_reason: Some("stop".into()),
                                        }],
                                        usage: None,
                                    })).await;
                                    return;
                                }
                                if let Ok(chunk) = serde_json::from_str::<OpenRouterStreamChunk>(data) {
                                    if let Some(choice) = chunk.choices.first() {
                                        if let Some(content) = &choice.delta.content {
                                            full_content.push_str(content);
                                            let _ = tx.send(TokenEvent::Token(content.clone())).await;
                                        }
                                        if choice.finish_reason.is_some() {
                                            let _ = tx.send(TokenEvent::Done(ChatResponse {
                                                id: chunk.id,
                                                model: chunk.model,
                                                choices: vec![crate::llm::provider::ChatChoice {
                                                    index: 0,
                                                    message: crate::llm::provider::ChatMessage { role: "assistant".into(), content: full_content.clone(), name: None },
                                                    finish_reason: choice.finish_reason.clone(),
                                                }],
                                                usage: None,
                                            })).await;
                                            return;
                                        }
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
        let messages: Vec<OpenRouterMessage> = req.messages.into_iter().map(|m| OpenRouterMessage {
            role: m.role,
            content: m.content,
        }).collect();

        let request = OpenRouterRequest {
            model: req.model,
            messages,
            max_tokens: req.max_tokens,
            temperature: req.temperature,
            stream: false,
        };

        let resp = self.client.post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await
            .map_err(|e| LlmError::ApiError(e.to_string()))?;

        let data: OpenRouterResponse = resp.json().await
            .map_err(|e| LlmError::ApiError(e.to_string()))?;

        let choice = data.choices.first().ok_or(LlmError::Internal("No choices".into()))?;
        Ok(ChatResponse {
            id: data.id,
            model: data.model,
            choices: vec![crate::llm::provider::ChatChoice {
                index: 0,
                message: crate::llm::provider::ChatMessage {
                    role: "assistant".to_string(),
                    content: choice.message.content.clone(),
                    name: None,
                },
                finish_reason: choice.finish_reason.clone(),
            }],
            usage: data.usage.map(|u| crate::llm::provider::Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            }),
        })
    }
}