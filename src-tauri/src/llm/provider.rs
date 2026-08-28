use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize, Error)]
pub enum LlmError {
    #[error("Provider not available: {0}")]
    ProviderUnavailable(String),
    #[error("API error: {0}")]
    ApiError(String),
    #[error("Stream error: {0}")]
    StreamError(String),
    #[error("Model not found: {0}")]
    ModelNotFound(String),
    #[error("Authentication failed: {0}")]
    AuthFailed(String),
    #[error("Rate limited: {0}")]
    RateLimited(String),
    #[error("Timeout: {0}")]
    Timeout(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stream: bool,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatChoice {
    pub index: u32,
    pub message: ChatMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub context_window: u32,
    pub capabilities: Vec<ModelCapability>,
    pub pricing: Option<ModelPricing>,
    pub local_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ModelCapability {
    Chat,
    Completion,
    Embedding,
    Vision,
    Audio,
    ToolUse,
    Reasoning,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricing {
    pub input_per_1k: f64,
    pub output_per_1k: f64,
    pub currency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TokenEvent {
    Token(String),
    Done(ChatResponse),
    Error(String),
}

pub type TokenStream = mpsc::Receiver<TokenEvent>;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat_stream(&self, req: ChatRequest) -> Result<TokenStream, LlmError>;
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, LlmError>;
    async fn list_models(&self) -> Result<Vec<ModelInfo>, LlmError>;
    async fn health_check(&self) -> Result<(), LlmError>;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn supports_streaming(&self) -> bool { true }
    fn supported_capabilities(&self) -> Vec<ModelCapability> { vec![ModelCapability::Chat] }
}