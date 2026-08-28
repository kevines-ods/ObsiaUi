use crate::llm::provider::{
    ChatRequest, ChatResponse, LlmError, LlmProvider, ModelCapability, ModelInfo, ModelPricing,
    TokenEvent, TokenStream,
};
use async_openai::{
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestMessage, ChatCompletionRequestUserMessageArgs,
        CreateChatCompletionRequestArgs, Role,
    },
    Client as OpenAIClient,
};
use async_trait::async_trait;
use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, instrument};

pub struct OpenAIProvider {
    client: Arc<OpenAIClient<OpenAIConfig>>,
    api_key: String,
}

impl OpenAIProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        let key = api_key.into();
        let config = OpenAIConfig::new().with_api_key(key.clone());
        let client = Arc::new(OpenAIClient::with_config(config));
        Self {
            client,
            api_key: key,
        }
    }

    pub fn from_env() -> Result<Self, LlmError> {
        let key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| LlmError::AuthFailed("OPENAI_API_KEY not set".into()))?;
        Ok(Self::new(key))
    }
}

#[async_trait]
impl LlmProvider for OpenAIProvider {
    fn id(&self) -> &str {
        "openai"
    }
    fn name(&self) -> &str {
        "OpenAI"
    }

    #[instrument(skip(self))]
    async fn health_check(&self) -> Result<(), LlmError> {
        let req = CreateChatCompletionRequestArgs::default()
            .model("gpt-3.5-turbo")
            .max_tokens(1u16)
            .messages(vec![ChatCompletionRequestUserMessageArgs::default()
                .content("ping")
                .build()
                .unwrap()
                .into()])
            .build()
            .map_err(|e| LlmError::Internal(e.to_string()))?;
        self.client
            .chat()
            .create(req)
            .await
            .map(|_| {
                info!("OpenAI health check OK");
            })
            .map_err(|e| LlmError::ProviderUnavailable(e.to_string()))
    }

    #[instrument(skip(self))]
    async fn list_models(&self) -> Result<Vec<ModelInfo>, LlmError> {
        let models = self
            .client
            .models()
            .list()
            .await
            .map_err(|e| LlmError::ApiError(e.to_string()))?;
        Ok(models
            .data
            .into_iter()
            .filter_map(|m| {
                let id = m.id;
                if id.starts_with("gpt") || id.starts_with("o1") {
                    let (context, pricing) = match id.as_str() {
                        "gpt-4o" | "gpt-4o-2024-08-06" => (
                            128_000,
                            Some(ModelPricing {
                                input_per_1k: 0.005,
                                output_per_1k: 0.015,
                                currency: "USD".into(),
                            }),
                        ),
                        "gpt-4o-mini" => (
                            128_000,
                            Some(ModelPricing {
                                input_per_1k: 0.00015,
                                output_per_1k: 0.0006,
                                currency: "USD".into(),
                            }),
                        ),
                        "gpt-4-turbo" => (
                            128_000,
                            Some(ModelPricing {
                                input_per_1k: 0.01,
                                output_per_1k: 0.03,
                                currency: "USD".into(),
                            }),
                        ),
                        "gpt-3.5-turbo" => (
                            16_385,
                            Some(ModelPricing {
                                input_per_1k: 0.0005,
                                output_per_1k: 0.0015,
                                currency: "USD".into(),
                            }),
                        ),
                        "o1-preview" => (
                            128_000,
                            Some(ModelPricing {
                                input_per_1k: 0.015,
                                output_per_1k: 0.06,
                                currency: "USD".into(),
                            }),
                        ),
                        "o1-mini" => (
                            128_000,
                            Some(ModelPricing {
                                input_per_1k: 0.003,
                                output_per_1k: 0.012,
                                currency: "USD".into(),
                            }),
                        ),
                        _ => (4096, None),
                    };
                    Some(ModelInfo {
                        id: id.clone(),
                        name: id,
                        provider: "openai".to_string(),
                        context_window: context,
                        capabilities: vec![ModelCapability::Chat, ModelCapability::ToolUse],
                        pricing,
                        local_path: None,
                    })
                } else {
                    None
                }
            })
            .collect())
    }

    #[instrument(skip(self, req))]
    async fn chat_stream(&self, req: ChatRequest) -> Result<TokenStream, LlmError> {
        let (tx, rx) = mpsc::channel(100);
        let client = self.client.clone();
        let model = req.model.clone();
        let messages: Vec<ChatCompletionRequestMessage> = req
            .messages
            .into_iter()
            .map(|m| {
                let role = match m.role.as_str() {
                    "system" => Role::System,
                    "user" => Role::User,
                    "assistant" => Role::Assistant,
                    _ => Role::User,
                };
                ChatCompletionRequestUserMessageArgs::default()
                    .role(role)
                    .content(m.content)
                    .build()
                    .unwrap()
                    .into()
            })
            .collect();

        tokio::spawn(async move {
            let mut binding = CreateChatCompletionRequestArgs::default();
            let mut request_builder = binding
                .model(model)
                .messages(messages)
                .temperature(req.temperature.unwrap_or(1.0));

            if let Some(max_t) = req.max_tokens.and_then(|t| u16::try_from(t).ok()) {
                request_builder = request_builder.max_tokens(max_t);
            }

            let request = request_builder
                .build()
                .map_err(|e| LlmError::Internal(e.to_string()));

            let request = match request {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(TokenEvent::Error(e.to_string())).await;
                    return;
                }
            };

            match client.chat().create_stream(request).await {
                Ok(mut stream) => {
                    let mut full_content = String::new();
                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Ok(c) => {
                                if let Some(choice) = c.choices.first() {
                                    if let Some(content) = &choice.delta.content {
                                        full_content.push_str(content);
                                        let _ = tx.send(TokenEvent::Token(content.clone())).await;
                                    }
                                    if choice.finish_reason.is_some() {
                                        let _ = tx
                                            .send(TokenEvent::Done(ChatResponse {
                                                id: c.id,
                                                model: c.model,
                                                choices: vec![crate::llm::provider::ChatChoice {
                                                    index: 0,
                                                    message: crate::llm::provider::ChatMessage {
                                                        role: "assistant".to_string(),
                                                        content: full_content,
                                                        name: None,
                                                    },
                                                    finish_reason: choice
                                                        .finish_reason
                                                        .as_ref()
                                                        .map(|f| format!("{:?}", f)),
                                                }],
                                                usage: None,
                                            }))
                                            .await;
                                        break;
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(TokenEvent::Error(e.to_string())).await;
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(TokenEvent::Error(e.to_string())).await;
                }
            }
        });

        Ok(rx)
    }

    #[instrument(skip(self, req))]
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, LlmError> {
        let messages: Vec<ChatCompletionRequestMessage> = req
            .messages
            .into_iter()
            .map(|m| {
                let role = match m.role.as_str() {
                    "system" => Role::System,
                    "user" => Role::User,
                    "assistant" => Role::Assistant,
                    _ => Role::User,
                };
                ChatCompletionRequestUserMessageArgs::default()
                    .role(role)
                    .content(m.content)
                    .build()
                    .unwrap()
                    .into()
            })
            .collect();

        let mut binding = CreateChatCompletionRequestArgs::default();
        let mut request_builder = binding
            .model(req.model)
            .messages(messages)
            .temperature(req.temperature.unwrap_or(1.0));

        if let Some(max_t) = req.max_tokens.and_then(|t| u16::try_from(t).ok()) {
            request_builder = request_builder.max_tokens(max_t);
        }

        let request = request_builder
            .build()
            .map_err(|e| LlmError::Internal(e.to_string()))?;

        let resp = self
            .client
            .chat()
            .create(request)
            .await
            .map_err(|e| LlmError::ApiError(e.to_string()))?;

        let choice = resp
            .choices
            .first()
            .ok_or(LlmError::Internal("No choices".into()))?;
        Ok(ChatResponse {
            id: resp.id,
            model: resp.model,
            choices: vec![crate::llm::provider::ChatChoice {
                index: 0,
                message: crate::llm::provider::ChatMessage {
                    role: "assistant".to_string(),
                    content: choice.message.content.clone().unwrap_or_default(),
                    name: None,
                },
                finish_reason: choice.finish_reason.as_ref().map(|f| format!("{:?}", f)),
            }],
            usage: resp.usage.map(|u| crate::llm::provider::Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            }),
        })
    }
}
