use crate::llm::provider::{
    ChatRequest, ChatResponse, LlmError, LlmProvider, ModelCapability, ModelInfo, TokenEvent,
    TokenStream,
};
use async_trait::async_trait;
use ollama_rs::{generation::chat::ChatMessage as OllamaChatMessage, Ollama};
use reqwest::Client;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, instrument};

pub struct OllamaProvider {
    client: Arc<Ollama>,
    base_url: String,
}

impl OllamaProvider {
    pub fn new(base_url: impl Into<String>) -> Self {
        let url = base_url.into();
        let client = Arc::new(Ollama::default());
        Self {
            client,
            base_url: url,
        }
    }

    pub fn from_env() -> Self {
        Self::new(std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost".to_string()))
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn id(&self) -> &str {
        "ollama"
    }
    fn name(&self) -> &str {
        "Ollama (Local)"
    }

    #[instrument(skip(self))]
    async fn health_check(&self) -> Result<(), LlmError> {
        let client = Client::new();
        let resp = client
            .get(format!("{}/api/tags", self.base_url))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| LlmError::ProviderUnavailable(e.to_string()))?;
        if resp.status().is_success() {
            info!("Ollama health check OK");
            Ok(())
        } else {
            Err(LlmError::ProviderUnavailable(
                "Ollama not responding".into(),
            ))
        }
    }

    #[instrument(skip(self))]
    async fn list_models(&self) -> Result<Vec<ModelInfo>, LlmError> {
        let client = Client::new();
        let resp = client
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .await
            .map_err(|e| LlmError::ApiError(e.to_string()))?;
        let data: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| LlmError::ApiError(e.to_string()))?;
        let models = data["models"].as_array().cloned().unwrap_or_default();
        Ok(models
            .iter()
            .filter_map(|m| {
                m["name"].as_str().map(|name| ModelInfo {
                    id: name.to_string(),
                    name: name.to_string(),
                    provider: "ollama".to_string(),
                    context_window: 4096,
                    capabilities: vec![ModelCapability::Chat],
                    pricing: None,
                    local_path: None,
                })
            })
            .collect())
    }

    #[instrument(skip(self, req))]
    async fn chat_stream(&self, req: ChatRequest) -> Result<TokenStream, LlmError> {
        let (tx, rx) = mpsc::channel(100);
        let client = self.client.clone();
        let model = req.model.clone();
        let messages: Vec<OllamaChatMessage> = req
            .messages
            .into_iter()
            .map(|m| {
                OllamaChatMessage::new(
                    match m.role.as_str() {
                        "system" => ollama_rs::generation::chat::MessageRole::System,
                        "assistant" => ollama_rs::generation::chat::MessageRole::Assistant,
                        _ => ollama_rs::generation::chat::MessageRole::User,
                    },
                    m.content,
                )
            })
            .collect();

        tokio::spawn(async move {
            use ollama_rs::generation::chat::request::ChatMessageRequest;
            let request = ChatMessageRequest::new(model, messages);
            match client.send_chat_messages(request).await {
                Ok(resp) => {
                    let _ = tx.send(TokenEvent::Token(resp.message.content)).await;
                    let _ = tx
                        .send(TokenEvent::Done(ChatResponse {
                            id: uuid::Uuid::new_v4().to_string(),
                            model: resp.model,
                            choices: vec![],
                            usage: None,
                        }))
                        .await;
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
        let messages: Vec<OllamaChatMessage> = req
            .messages
            .into_iter()
            .map(|m| {
                OllamaChatMessage::new(
                    match m.role.as_str() {
                        "system" => ollama_rs::generation::chat::MessageRole::System,
                        "assistant" => ollama_rs::generation::chat::MessageRole::Assistant,
                        _ => ollama_rs::generation::chat::MessageRole::User,
                    },
                    m.content,
                )
            })
            .collect();

        use ollama_rs::generation::chat::request::ChatMessageRequest;
        let request = ChatMessageRequest::new(req.model, messages);
        let resp = self
            .client
            .send_chat_messages(request)
            .await
            .map_err(|e| LlmError::ApiError(e.to_string()))?;

        Ok(ChatResponse {
            id: uuid::Uuid::new_v4().to_string(),
            model: resp.model,
            choices: vec![crate::llm::provider::ChatChoice {
                index: 0,
                message: crate::llm::provider::ChatMessage {
                    role: "assistant".to_string(),
                    content: resp.message.content,
                    name: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: None,
        })
    }
}
