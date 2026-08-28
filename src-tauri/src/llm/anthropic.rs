use crate::llm::provider::{
    ChatRequest, ChatResponse, LlmError, LlmProvider, ModelCapability, ModelInfo, ModelPricing,
    TokenEvent, TokenStream,
};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, instrument};

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    max_tokens: u32,
    temperature: Option<f32>,
    stream: bool,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    id: String,
    content: Vec<AnthropicContent>,
    model: String,
    stop_reason: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Deserialize)]
struct AnthropicContent {
    text: String,
}

#[derive(Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

#[derive(Deserialize)]
struct AnthropicStreamEvent {
    r#type: String,
    delta: Option<AnthropicDelta>,
}

#[derive(Deserialize)]
struct AnthropicDelta {
    text: Option<String>,
}

pub struct AnthropicProvider {
    client: Arc<Client>,
    api_key: String,
    base_url: String,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        let key = api_key.into();
        let client = Arc::new(Client::new());
        Self {
            client,
            api_key: key,
            base_url: "https://api.anthropic.com".to_string(),
        }
    }

    pub fn from_env() -> Result<Self, LlmError> {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| LlmError::AuthFailed("ANTHROPIC_API_KEY not set".into()))?;
        Ok(Self::new(key))
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn id(&self) -> &str {
        "anthropic"
    }
    fn name(&self) -> &str {
        "Anthropic"
    }

    #[instrument(skip(self))]
    async fn health_check(&self) -> Result<(), LlmError> {
        let req = AnthropicRequest {
            model: "claude-3-haiku-20240307".to_string(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: "ping".to_string(),
            }],
            max_tokens: 1,
            temperature: None,
            stream: false,
        };
        let resp = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&req)
            .send()
            .await
            .map_err(|e| LlmError::ProviderUnavailable(e.to_string()))?;
        if resp.status().is_success() {
            info!("Anthropic health check OK");
            Ok(())
        } else {
            Err(LlmError::ProviderUnavailable(
                "Anthropic not responding".into(),
            ))
        }
    }

    #[instrument(skip(self))]
    async fn list_models(&self) -> Result<Vec<ModelInfo>, LlmError> {
        Ok(vec![
            ModelInfo {
                id: "claude-3-5-sonnet-20241022".into(),
                name: "Claude 3.5 Sonnet".into(),
                provider: "anthropic".into(),
                context_window: 200_000,
                capabilities: vec![
                    ModelCapability::Chat,
                    ModelCapability::ToolUse,
                    ModelCapability::Vision,
                    ModelCapability::Reasoning,
                ],
                pricing: Some(ModelPricing {
                    input_per_1k: 0.003,
                    output_per_1k: 0.015,
                    currency: "USD".into(),
                }),
                local_path: None,
            },
            ModelInfo {
                id: "claude-3-5-haiku-20241022".into(),
                name: "Claude 3.5 Haiku".into(),
                provider: "anthropic".into(),
                context_window: 200_000,
                capabilities: vec![ModelCapability::Chat, ModelCapability::ToolUse],
                pricing: Some(ModelPricing {
                    input_per_1k: 0.0008,
                    output_per_1k: 0.004,
                    currency: "USD".into(),
                }),
                local_path: None,
            },
            ModelInfo {
                id: "claude-3-opus-20240229".into(),
                name: "Claude 3 Opus".into(),
                provider: "anthropic".into(),
                context_window: 200_000,
                capabilities: vec![
                    ModelCapability::Chat,
                    ModelCapability::ToolUse,
                    ModelCapability::Vision,
                    ModelCapability::Reasoning,
                ],
                pricing: Some(ModelPricing {
                    input_per_1k: 0.015,
                    output_per_1k: 0.075,
                    currency: "USD".into(),
                }),
                local_path: None,
            },
        ])
    }

    #[instrument(skip(self, req))]
    async fn chat_stream(&self, req: ChatRequest) -> Result<TokenStream, LlmError> {
        let (tx, rx) = mpsc::channel(100);
        let client = self.client.clone();
        let api_key = self.api_key.clone();
        let base_url = self.base_url.clone();
        let model = req.model.clone();
        let messages: Vec<AnthropicMessage> = req
            .messages
            .into_iter()
            .map(|m| AnthropicMessage {
                role: if m.role == "assistant" {
                    "assistant"
                } else {
                    "user"
                }
                .to_string(),
                content: m.content,
            })
            .collect();
        let max_tokens = req.max_tokens.unwrap_or(4096);

        tokio::spawn(async move {
            let request = AnthropicRequest {
                model,
                messages,
                max_tokens,
                temperature: req.temperature,
                stream: true,
            };

            let resp = client
                .post(format!("{}/v1/messages", base_url))
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&request)
                .send()
                .await;

            let resp = match resp {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(TokenEvent::Error(e.to_string())).await;
                    return;
                }
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
                            if let Some(data) = line.strip_prefix("data: ") {
                                if data == "[DONE]" {
                                    let _ = tx
                                        .send(TokenEvent::Done(ChatResponse {
                                            id: uuid::Uuid::new_v4().to_string(),
                                            model: request.model.clone(),
                                            choices: vec![crate::llm::provider::ChatChoice {
                                                index: 0,
                                                message: crate::llm::provider::ChatMessage {
                                                    role: "assistant".into(),
                                                    content: full_content,
                                                    name: None,
                                                },
                                                finish_reason: Some("stop".into()),
                                            }],
                                            usage: None,
                                        }))
                                        .await;
                                    return;
                                }
                                if let Ok(event) =
                                    serde_json::from_str::<AnthropicStreamEvent>(data)
                                {
                                    if let Some(delta) = event.delta {
                                        if let Some(text) = delta.text {
                                            full_content.push_str(&text);
                                            let _ = tx.send(TokenEvent::Token(text)).await;
                                        }
                                    }
                                }
                            }
                        }
                        buffer.clear();
                    }
                    Err(e) => {
                        let _ = tx.send(TokenEvent::Error(e.to_string())).await;
                        return;
                    }
                }
            }
        });

        Ok(rx)
    }

    #[instrument(skip(self, req))]
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, LlmError> {
        let messages: Vec<AnthropicMessage> = req
            .messages
            .into_iter()
            .map(|m| AnthropicMessage {
                role: if m.role == "assistant" {
                    "assistant"
                } else {
                    "user"
                }
                .to_string(),
                content: m.content,
            })
            .collect();

        let request = AnthropicRequest {
            model: req.model,
            messages,
            max_tokens: req.max_tokens.unwrap_or(4096),
            temperature: req.temperature,
            stream: false,
        };

        let resp = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&request)
            .send()
            .await
            .map_err(|e| LlmError::ApiError(e.to_string()))?;

        let data: AnthropicResponse = resp
            .json()
            .await
            .map_err(|e| LlmError::ApiError(e.to_string()))?;

        Ok(ChatResponse {
            id: data.id,
            model: data.model,
            choices: vec![crate::llm::provider::ChatChoice {
                index: 0,
                message: crate::llm::provider::ChatMessage {
                    role: "assistant".to_string(),
                    content: data
                        .content
                        .first()
                        .map(|c| c.text.clone())
                        .unwrap_or_default(),
                    name: None,
                },
                finish_reason: data.stop_reason,
            }],
            usage: Some(crate::llm::provider::Usage {
                prompt_tokens: data.usage.input_tokens,
                completion_tokens: data.usage.output_tokens,
                total_tokens: data.usage.input_tokens + data.usage.output_tokens,
            }),
        })
    }
}
