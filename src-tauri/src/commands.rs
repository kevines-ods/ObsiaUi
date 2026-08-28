use crate::llm::{
    ChatRequest, ChatResponse, ModelInfo,
    ProviderRegistry, ModelRegistry, ProviderPool, PoolStrategy,
    StreamingManager,
};
use tauri::{AppHandle, State};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::instrument;

#[derive(Debug, Deserialize)]
pub struct ChatRequestPayload {
    pub model: String,
    pub messages: Vec<ChatMessagePayload>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub provider: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ChatMessagePayload {
    pub role: String,
    pub content: String,
}

#[derive(Serialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub models: Vec<ModelInfo>,
}

type ProviderRegistryState = Arc<ProviderRegistry>;
type ModelRegistryState = Arc<ModelRegistry>;
type ProviderPoolState = Arc<ProviderPool>;

#[tauri::command]
#[instrument(skip(provider_registry))]
pub async fn list_providers(
    provider_registry: State<'_, ProviderRegistryState>,
) -> Result<Vec<ProviderInfo>, String> {
    let providers = provider_registry.list_all().await;
    let mut result = Vec::new();
    for provider in providers {
        let models = provider.list_models().await.unwrap_or_default();
        result.push(ProviderInfo {
            id: provider.id().to_string(),
            name: provider.name().to_string(),
            models,
        });
    }
    Ok(result)
}

#[tauri::command]
#[instrument(skip(app_handle, provider_pool))]
pub async fn chat_stream(
    app_handle: AppHandle,
    provider_pool: State<'_, ProviderPoolState>,
    request: ChatRequestPayload,
) -> Result<(), String> {
    let streaming = StreamingManager::new(app_handle.clone());
    
    let chat_req = ChatRequest {
        model: request.model,
        messages: request.messages.into_iter().map(|m| crate::llm::provider::ChatMessage {
            role: m.role,
            content: m.content,
            name: None,
        }).collect(),
        temperature: request.temperature,
        max_tokens: request.max_tokens,
        stream: true,
        metadata: Default::default(),
    };

    let stream = provider_pool.chat_stream_with_fallback(chat_req).await
        .map_err(|e| e.to_string())?;

    streaming.forward_stream(stream).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[instrument(skip(provider_pool))]
pub async fn chat(
    provider_pool: State<'_, ProviderPoolState>,
    request: ChatRequestPayload,
) -> Result<ChatResponse, String> {
    let chat_req = ChatRequest {
        model: request.model,
        messages: request.messages.into_iter().map(|m| crate::llm::provider::ChatMessage {
            role: m.role,
            content: m.content,
            name: None,
        }).collect(),
        temperature: request.temperature,
        max_tokens: request.max_tokens,
        stream: false,
        metadata: Default::default(),
    };

    provider_pool.chat_with_fallback(chat_req).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[instrument(skip(provider_registry))]
pub async fn llm_list_models(
    provider_registry: State<'_, ProviderRegistryState>,
    provider_id: Option<String>,
) -> Result<Vec<ModelInfo>, String> {
    if let Some(id) = provider_id {
        if let Some(provider) = provider_registry.get(&id).await {
            return provider.list_models().await.map_err(|e| e.to_string());
        }
        return Err("Provider not found".into());
    }
    let mut all_models = Vec::new();
    for provider in provider_registry.list_all().await {
        if let Ok(models) = provider.list_models().await {
            all_models.extend(models);
        }
    }
    Ok(all_models)
}

#[tauri::command]
#[instrument(skip(provider_registry))]
pub async fn llm_health_check(
    provider_registry: State<'_, ProviderRegistryState>,
) -> Result<serde_json::Value, String> {
    let results = provider_registry.health_check_all().await;
    Ok(serde_json::to_value(results).unwrap())
}

#[tauri::command]
#[instrument(skip(model_registry))]
pub async fn scan_local_models(
    model_registry: State<'_, ModelRegistryState>,
) -> Result<usize, String> {
    model_registry.scan_local_models().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_vault(_path: String) -> Result<Vec<serde_json::Value>, String> {
    Ok(vec![])
}

#[tauri::command]
pub async fn get_backlinks(_path: String) -> Result<Vec<String>, String> {
    Ok(vec![])
}

pub fn init_provider_registry() -> ProviderRegistryState {
    let registry = Arc::new(ProviderRegistry::new());
    
    // Ollama (local)
    let ollama = OllamaProvider::from_env();
    let registry_clone = registry.clone();
    tauri::async_runtime::spawn(async move {
        registry_clone.register(Arc::new(ollama)).await;
    });

    // OpenAI
    if let Ok(openai) = OpenAIProvider::from_env() {
        let registry_clone = registry.clone();
        tauri::async_runtime::spawn(async move {
            registry_clone.register(Arc::new(openai)).await;
        });
    }

    // Anthropic
    if let Ok(anthropic) = AnthropicProvider::from_env() {
        let registry_clone = registry.clone();
        tauri::async_runtime::spawn(async move {
            registry_clone.register(Arc::new(anthropic)).await;
        });
    }

    // OpenRouter
    if let Ok(openrouter) = OpenRouterProvider::from_env() {
        let registry_clone = registry.clone();
        tauri::async_runtime::spawn(async move {
            registry_clone.register(Arc::new(openrouter)).await;
        });
    }

    // Gemini
    if let Ok(gemini) = GeminiProvider::from_env() {
        let registry_clone = registry.clone();
        tauri::async_runtime::spawn(async move {
            registry_clone.register(Arc::new(gemini)).await;
        });
    }

    registry
}

pub fn init_model_registry() -> ModelRegistryState {
    let local_dirs = vec![
        dirs::home_dir().map(|h| h.join(".ollama/models")).unwrap_or_default(),
        dirs::home_dir().map(|h| h.join(".cache/lm-studio/models")).unwrap_or_default(),
        dirs::home_dir().map(|h| h.join("Models")).unwrap_or_default(),
    ];
    Arc::new(ModelRegistry::new(local_dirs))
}

pub fn init_provider_pool(_registry: ProviderRegistryState) -> ProviderPoolState {
    Arc::new(ProviderPool::new(PoolStrategy::Fallback))
}

use crate::llm::{OllamaProvider, OpenAIProvider, AnthropicProvider, OpenRouterProvider, GeminiProvider};