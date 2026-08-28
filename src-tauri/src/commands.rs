//! Contrat IPC Tauri — commandes exposées au frontend React.
//!
//! Noms de commandes (alignés sur `src/hooks/useLlmStream.ts`) :
//! - Chat : `chat_send`, `chat_stream`
//! - Providers : `providers_list`, `provider_test`, `models_list`,
//!   `llm_health_check`, `scan_local_models`
//! - Config : `config_get`, `config_set`
//! - Coffre : `vault_list`, `vault_read`, `vault_write`, `vault_path`
//!
//! Événements de stream (rétro-compatibles) : `llm:token`, `llm:done`, `llm:error`.

use crate::config::{ConfigPatch, ConfigState, ConfigView};
use crate::llm::fallback::{PoolStrategy, ProviderPool};
use crate::llm::provider::{ChatMessage, ChatRequest, ChatResponse, ModelInfo};
use crate::llm::registry::{ModelRegistry, ProviderRegistry};
use crate::llm::streaming::StreamingManager;
use crate::vault::{VaultEntry, VaultState};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tracing::{error, info, instrument};

// ===== Types du contrat =====

/// Payload d'entrée du chat (camelCase côté JS).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequestPayload {
    /// Fournisseur ciblé. `None` = fallback automatique sur le pool.
    pub provider: Option<String>,
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

/// Fournisseur + ses modèles (sortie de `providers_list`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub models: Vec<ModelInfo>,
}

/// État de santé d'un fournisseur (sortie de `provider_test`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderHealth {
    pub provider_id: String,
    pub ok: bool,
    pub error: Option<String>,
}

// ===== États Tauri (State) =====

pub type ProviderRegistryState = Arc<ProviderRegistry>;
pub type ModelRegistryState = Arc<ModelRegistry>;
pub type ProviderPoolState = Arc<ProviderPool>;

// ===== Initialisation (appelée depuis setup dans lib.rs) =====

/// Enregistre les providers : Ollama toujours + providers API si une clé
/// est dispo (env var prioritaire, sinon config fichier).
pub fn init_provider_registry(config: &ConfigState) -> ProviderRegistryState {
    let registry = Arc::new(ProviderRegistry::new());
    let cfg = config.read();

    // Ollama : toujours disponible (local) — host personnalisable via config
    let ollama = match &cfg.ollama_host {
        Some(host) => crate::llm::ollama::OllamaProvider::new(host.clone()),
        None => crate::llm::ollama::OllamaProvider::from_env(),
    };
    tauri::async_runtime::block_on(registry.register(Arc::new(ollama)));

    // Providers API : clé env var OU config
    let register_api = |provider_id: &str, key: Option<String>| {
        if let Some(key) = key {
            let provider: Option<Arc<dyn crate::llm::provider::LlmProvider>> = match provider_id {
                "openai" => Some(Arc::new(crate::llm::openai::OpenAIProvider::new(key))),
                "anthropic" => Some(Arc::new(crate::llm::anthropic::AnthropicProvider::new(key))),
                "openrouter" => Some(Arc::new(crate::llm::openrouter::OpenRouterProvider::new(key))),
                "gemini" => Some(Arc::new(crate::llm::gemini::GeminiProvider::new(key))),
                _ => None,
            };
            if let Some(p) = provider {
                tauri::async_runtime::block_on(registry.register(p));
                info!(provider_id, "provider API enregistré");
            }
        }
    };
    register_api("openai", cfg.api_key_for("openai"));
    register_api("anthropic", cfg.api_key_for("anthropic"));
    register_api("openrouter", cfg.api_key_for("openrouter"));
    register_api("gemini", cfg.api_key_for("gemini"));

    registry
}

/// Registry des modèles locaux (GGUF/safetensors).
pub fn init_model_registry() -> ModelRegistryState {
    let mut dirs = Vec::new();
    if let Ok(p) = std::env::var("OBSI_MODEL_DIR") {
        dirs.push(std::path::PathBuf::from(p));
    }
    Arc::new(ModelRegistry::new(dirs))
}

/// Pool de fournisseurs : synchronisé depuis le registry (corrige le bug
/// du pool vide — le pool et le registry étaient décorrélés).
pub fn init_provider_pool(registry: &ProviderRegistryState) -> ProviderPoolState {
    let pool = Arc::new(ProviderPool::new(PoolStrategy::Fallback));
    let mut count = 0usize;
    tauri::async_runtime::block_on(async {
        for p in registry.list_all().await {
            pool.add_provider(p).await;
            count += 1;
        }
    });
    info!(count, "pool initialisé");
    pool
}

/// Résout la racine du coffre (config > env > défauts dev).
pub fn init_vault(config: &ConfigState) -> Result<VaultState, String> {
    let vault_path = config.read().vault_path.clone();
    VaultState::resolve(vault_path)
}

// ===== Commandes — Chat =====

/// Chat non-streaming : réponse complète.
#[tauri::command]
#[instrument(skip(_app, pool))]
pub async fn chat_send(
    _app: AppHandle,
    pool: State<'_, ProviderPoolState>,
    payload: ChatRequestPayload,
) -> Result<ChatResponse, String> {
    validate_payload(&payload)?;
    let req = build_request(&payload, false);
    let result = match &payload.provider {
        Some(pid) => pool.chat_with(pid, req).await,
        None => pool.chat_with_fallback(req).await,
    };
    result.map_err(|e| {
        error!(%e, "chat_send échec");
        e.to_string()
    })
}

/// Chat streaming : émet `llm:token` / `llm:done` / `llm:error` puis
/// retourne `Ok(())` (le contenu transite par les événements).
#[tauri::command]
#[instrument(skip(app, pool))]
pub async fn chat_stream(
    app: AppHandle,
    pool: State<'_, ProviderPoolState>,
    payload: ChatRequestPayload,
) -> Result<(), String> {
    validate_payload(&payload)?;
    let req = build_request(&payload, true);
    let stream = match &payload.provider {
        Some(pid) => pool.chat_stream_with(pid, req).await,
        None => pool.chat_stream_with_fallback(req).await,
    }
    .map_err(|e| {
        error!(%e, "chat_stream échec d'ouverture");
        let _ = app.emit("llm:error", e.to_string());
        e.to_string()
    })?;

    let manager = StreamingManager::new(app.clone());
    manager.forward_stream(stream).await.map_err(|e| {
        error!(%e, "chat_stream échec de forwarding");
        e.to_string()
    })?;
    Ok(())
}

fn validate_payload(payload: &ChatRequestPayload) -> Result<(), String> {
    if payload.model.trim().is_empty() {
        return Err("model requis".into());
    }
    if payload.messages.is_empty() {
        return Err("messages ne doit pas être vide".into());
    }
    if payload
        .messages
        .iter()
        .any(|m| m.content.trim().is_empty())
    {
        return Err("message vide refusé".into());
    }
    Ok(())
}

fn build_request(payload: &ChatRequestPayload, stream: bool) -> ChatRequest {
    ChatRequest {
        model: payload.model.clone(),
        messages: payload.messages.clone(),
        temperature: payload.temperature,
        max_tokens: payload.max_tokens,
        stream,
        metadata: HashMap::new(),
    }
}

// ===== Commandes — Providers =====

/// Liste les fournisseurs enregistrés + leurs modèles.
#[tauri::command]
#[instrument(skip(registry))]
pub async fn providers_list(
    registry: State<'_, ProviderRegistryState>,
) -> Result<Vec<ProviderInfo>, String> {
    let mut out = Vec::new();
    for p in registry.list_all().await {
        let models = p.list_models().await.unwrap_or_default();
        out.push(ProviderInfo {
            id: p.id().to_string(),
            name: p.id().to_string(),
            models,
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Ping un fournisseur : `health_check` + liste des modèles.
#[tauri::command]
#[instrument(skip(registry))]
pub async fn provider_test(
    registry: State<'_, ProviderRegistryState>,
    provider_id: String,
) -> Result<ProviderHealth, String> {
    let provider = registry
        .get(&provider_id)
        .await
        .ok_or_else(|| format!("provider inconnu : {provider_id}"))?;
    match provider.health_check().await {
        Ok(()) => Ok(ProviderHealth {
            provider_id,
            ok: true,
            error: None,
        }),
        Err(e) => Ok(ProviderHealth {
            provider_id,
            ok: false,
            error: Some(e.to_string()),
        }),
    }
}

/// Modèles disponibles, par provider (tous si `provider_id` omis).
#[tauri::command]
#[instrument(skip(registry))]
pub async fn models_list(
    registry: State<'_, ProviderRegistryState>,
    provider_id: Option<String>,
) -> Result<Vec<ModelInfo>, String> {
    let mut models = Vec::new();
    if let Some(pid) = provider_id {
        if let Some(p) = registry.get(&pid).await {
            models = p.list_models().await.map_err(|e| e.to_string())?;
        } else {
            return Err(format!("provider inconnu : {pid}"));
        }
    } else {
        for p in registry.list_all().await {
            if let Ok(m) = p.list_models().await {
                models.extend(m);
            }
        }
    }
    Ok(models)
}

/// Health-check global de tous les providers.
#[tauri::command]
#[instrument(skip(registry))]
pub async fn llm_health_check(
    registry: State<'_, ProviderRegistryState>,
) -> Result<Vec<ProviderHealth>, String> {
    let results = registry.health_check_all().await;
    let mut out: Vec<ProviderHealth> = results
        .into_iter()
        .map(|(provider_id, res)| ProviderHealth {
            provider_id,
            ok: res.is_ok(),
            error: res.err().map(|e| e.to_string()),
        })
        .collect();
    out.sort_by(|a, b| a.provider_id.cmp(&b.provider_id));
    Ok(out)
}

/// Scan des modèles locaux (GGUF/safetensors) dans les dossiers configurés.
#[tauri::command]
#[instrument(skip(model_registry))]
pub async fn scan_local_models(
    model_registry: State<'_, ModelRegistryState>,
) -> Result<Vec<ModelInfo>, String> {
    let count = model_registry
        .scan_local_models()
        .await
        .map_err(|e| e.to_string())?;
    info!(count, "modèles locaux scannés");
    Ok(model_registry.list_all().await)
}

// ===== Commandes — Config =====

/// Retourne la config SANS les valeurs de clés API (présence seulement).
#[tauri::command]
#[instrument(skip(config))]
pub fn config_get(config: State<'_, ConfigState>) -> Result<ConfigView, String> {
    Ok(config.read().view())
}

/// Applique un patch de config (clés API, chemin coffre, provider défaut).
#[tauri::command]
#[instrument(skip(config))]
pub fn config_set(
    config: State<'_, ConfigState>,
    patch: ConfigPatch,
) -> Result<ConfigView, String> {
    config.update(patch)
}

// ===== Commandes — Coffre (sandbox obsi_vault) =====

/// Liste les notes markdown du coffre.
#[tauri::command]
#[instrument(skip(vault))]
pub fn vault_list(vault: State<'_, VaultState>) -> Result<Vec<VaultEntry>, String> {
    vault.list_notes()
}

/// Lit une note markdown (chemin relatif, extension .md).
#[tauri::command]
#[instrument(skip(vault))]
pub fn vault_read(vault: State<'_, VaultState>, rel_path: String) -> Result<String, String> {
    vault.read_note(&rel_path)
}

/// Écrit une note markdown (chemin relatif, sandbox + protection).
#[tauri::command]
#[instrument(skip(vault))]
pub fn vault_write(
    vault: State<'_, VaultState>,
    rel_path: String,
    content: String,
) -> Result<VaultEntry, String> {
    vault.write_note(&rel_path, &content)
}

/// Chemin absolu de la racine du coffre (pour l'UI).
#[tauri::command]
#[instrument(skip(vault))]
pub fn vault_path(vault: State<'_, VaultState>) -> Result<String, String> {
    Ok(vault.root().display().to_string())
}

// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::fallback::{PoolStrategy, ProviderPool};
    use crate::llm::provider::{LlmError, LlmProvider, TokenEvent};
    use async_trait::async_trait;
    use tokio::sync::mpsc;

    /// Fake provider pour tests d'intégration (sans réseau).
    struct FakeProvider {
        id: String,
        models: Vec<ModelInfo>,
    }

    impl FakeProvider {
        fn new(id: &str, model_ids: &[&str]) -> Self {
            Self {
                id: id.to_string(),
                models: model_ids
                    .iter()
                    .map(|m| ModelInfo {
                        id: m.to_string(),
                        name: m.to_string(),
                        provider: id.to_string(),
                        context_window: 4096,
                        capabilities: vec![crate::llm::provider::ModelCapability::Chat],
                        pricing: None,
                        local_path: None,
                    })
                    .collect(),
            }
        }
    }

    #[async_trait]
    impl crate::llm::provider::LlmProvider for FakeProvider {
        async fn chat_stream(
            &self,
            req: ChatRequest,
        ) -> Result<tokio::sync::mpsc::Receiver<TokenEvent>, LlmError> {
            let (tx, rx) = mpsc::channel(10);
            for msg in &req.messages {
                tx.send(TokenEvent::Token(format!("echo:{}", msg.content)))
                    .await
                    .unwrap();
            }
            tx.send(TokenEvent::Done(self_echo_response(&req))).await.unwrap();
            Ok(rx)
        }

        async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, LlmError> {
            Ok(self_echo_response(&req))
        }

        async fn list_models(&self) -> Result<Vec<ModelInfo>, LlmError> {
            Ok(self.models.clone())
        }

        async fn health_check(&self) -> Result<(), LlmError> {
            Ok(())
        }

        fn id(&self) -> &str {
            &self.id
        }

        fn name(&self) -> &str {
            &self.id
        }
    }

    fn self_echo_response(req: &ChatRequest) -> ChatResponse {
        let content = req
            .messages
            .iter()
            .map(|m| m.content.clone())
            .collect::<Vec<_>>()
            .join(" ");
        ChatResponse {
            id: "fake-1".into(),
            model: req.model.clone(),
            choices: vec![crate::llm::provider::ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".into(),
                    content,
                    name: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: None,
        }
    }

    async fn pool_with_fake() -> ProviderPool {
        let pool = ProviderPool::new(PoolStrategy::Fallback);
        pool.add_provider(Arc::new(FakeProvider::new("fake", &["fake-model"])))
            .await;
        pool
    }

    #[tokio::test]
    async fn chat_send_fake_retourne_reponse_structuree() {
        let pool = pool_with_fake().await;
        let req = ChatRequest {
            model: "fake-model".into(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "bonjour".into(),
                name: None,
            }],
            temperature: None,
            max_tokens: None,
            stream: false,
            metadata: HashMap::new(),
        };
        let resp = pool.chat_with("fake", req).await.unwrap();
        assert_eq!(resp.model, "fake-model");
        assert_eq!(resp.choices[0].message.content, "bonjour");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[tokio::test]
    async fn chat_stream_fake_emmet_des_tokens() {
        let pool = pool_with_fake().await;
        let req = ChatRequest {
            model: "fake-model".into(),
            messages: vec![
                ChatMessage { role: "user".into(), content: "a".into(), name: None },
                ChatMessage { role: "user".into(), content: "b".into(), name: None },
            ],
            temperature: None,
            max_tokens: None,
            stream: true,
            metadata: HashMap::new(),
        };
        let mut rx = pool.chat_stream_with("fake", req).await.unwrap();
        let mut tokens = Vec::new();
        while let Some(ev) = rx.recv().await {
            match ev {
                TokenEvent::Token(t) => tokens.push(t),
                TokenEvent::Done(_) => break,
                TokenEvent::Error(_) => panic!("erreur inattendue"),
            }
        }
        assert_eq!(tokens, vec!["echo:a", "echo:b"]);
    }

    #[tokio::test]
    async fn provider_inconnu_refuse() {
        let pool = pool_with_fake().await;
        let req = ChatRequest {
            model: "fake-model".into(),
            messages: vec![ChatMessage { role: "user".into(), content: "x".into(), name: None }],
            temperature: None,
            max_tokens: None,
            stream: false,
            metadata: HashMap::new(),
        };
        let err = pool.chat_with("inexistant", req).await.unwrap_err();
        assert!(err.to_string().contains("inexistant"));
    }

    #[tokio::test]
    async fn providers_list_fake_retourne_la_liste_attendue() {
        let registry = Arc::new(ProviderRegistry::new());
        registry
            .register(Arc::new(FakeProvider::new("fake", &["fake-model"])))
            .await;
        let all = registry.list_all().await;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id(), "fake");
        let models = all[0].list_models().await.unwrap();
        assert_eq!(models[0].id, "fake-model");
        assert_eq!(models[0].provider, "fake");
    }

    /// Test d'intégration réel avec le daemon Ollama local.
    /// Exécution : `cargo test -- --ignored chat_ollama_reel`
    /// (nécessite `ollama serve` actif + le modèle qwen3.5:0.8b installé).
    #[tokio::test]
    #[ignore]
    async fn chat_ollama_reel() {
        let provider = crate::llm::ollama::OllamaProvider::from_env();
        let models = provider.list_models().await.expect("ollama joignable");
        assert!(
            models.iter().any(|m| m.id.contains("qwen3.5")),
            "le modèle qwen3.5:0.8b doit être installé — modèles: {:?}",
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>()
        );

        let req = ChatRequest {
            model: "qwen3.5:0.8b".into(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "Réponds uniquement par le mot OK".into(),
                name: None,
            }],
            temperature: Some(0.0),
            max_tokens: Some(32),
            stream: false,
            metadata: HashMap::new(),
        };
        let resp = provider.chat(req.clone()).await.expect("chat ollama");
        assert!(!resp.choices.is_empty());
        assert!(!resp.choices[0].message.content.trim().is_empty());

        // Streaming réel
        let req_stream = ChatRequest {
            stream: true,
            ..req
        };
        let mut rx = provider
            .chat_stream(req_stream)
            .await
            .expect("stream ollama");
        let mut chunks = 0usize;
        while let Some(ev) = rx.recv().await {
            match ev {
                crate::llm::provider::TokenEvent::Token(t) => {
                    assert!(!t.is_empty());
                    chunks += 1;
                }
                crate::llm::provider::TokenEvent::Done(_) => break,
                crate::llm::provider::TokenEvent::Error(e) => panic!("erreur stream: {e}"),
            }
        }
        assert!(chunks > 0, "le stream doit émettre au moins un chunk");
    }
}
