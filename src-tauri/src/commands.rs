//! Contrat IPC Tauri — commandes exposées au frontend React.
//!
//! Noms de commandes (alignés sur `src/hooks/useLlmStream.ts`) :
//! - Chat : `chat_send`, `chat_stream`
//! - Providers : `providers_list`, `provider_test`, `models_list`,
//!   `llm_health_check`, `scan_local_models`
//! - Config : `config_get`, `config_set`
//! - Coffre : `vault_list`, `vault_read`, `vault_write`, `vault_path`
//! - Agents : `agents_list`, `agent_read` (frontmatter validé par le backend)
//! - Runtimes locaux : `runtimes_detect` (Ollama / llama.cpp)
//! - Équipes : `teams_list`, `team_save`, `team_delete`, `team_run`
//! - Intendant : `intendant_prompt`, `intendant_send`, `intendant_apply`
//!   (la seule action qui écrit, `mcp`, vise `brouillon/`)
//! - Coffre : `vault_graph` (liens et étiquettes), `vault_open_external`
//! - MCP : `mcp_list`, `mcp_draft` (déclaration seulement — ObsiaUi ne
//!   se connecte pas aux serveurs MCP)
//! - Interface : `patches_list`, `patch_save`, `patch_delete`,
//!   `patch_toggle`, `patch_css`
//! - Plugins : `plugins_list`, `plugins_load`, `plugins_dir`,
//!   `plugin_enable`, `plugin_disable`
//! - Distant : `remote_status`, `remote_start`, `remote_stop`,
//!   `remote_token_read`, `remote_token_rotate`
//! - Plans : `plans_list`, `plan_save`, `plan_delete`, `plan_draft`,
//!   `plan_run`, `plan_cancel`
//! - Sessions : `sessions_list`, `session_create`, `session_get`,
//!   `session_rename`, `session_delete`, `session_send`, `session_cancel`,
//!   `session_export`
//!
//! Événements de stream :
//! - par session : `session:token`, `session:done`, `session:error` — la
//!   charge utile porte `sessionId`, ce qui permet à l'interface de
//!   s'abonner une fois pour toutes les sessions ouvertes ;
//! - hérités, pour le chat sans session : `llm:token`, `llm:done`,
//!   `llm:error`.

use crate::agents::{AgentDoc, AgentInfo};
use crate::config::{AppConfig, ConfigPatch, ConfigState, ConfigView};
use crate::discovery::{self, RuntimeKind, RuntimeScan};
use crate::event::EventBus;
use crate::graph::VaultGraph;
use crate::intendant;
use crate::llm::fallback::{PoolStrategy, ProviderPool};
use crate::llm::provider::TokenEvent;
use crate::llm::provider::{ChatMessage, ChatRequest, ChatResponse, LlmProvider, ModelInfo};
use crate::llm::registry::{ModelRegistry, ProviderRegistry};
use crate::mcp::McpInfo;
use crate::plan::{self, Plan, PlanManager, PlanStatus, PlanStep};
use crate::plugin::{self, InstalledPlugin, LayoutPatch, PluginStore, UiPatch};
use crate::remote::{self, Harness, RemoteState};
use crate::session::{Session, SessionManager, SessionMessage, SessionMeta};
use crate::team::{self, Team, TeamMember, TeamStore, TeamStrategy};
use crate::vault::{VaultEntry, VaultState};

/// Coffre et config sont partagés : la fenêtre Tauri et le serveur distant
/// s'appuient sur les mêmes instances, pas sur deux vues divergentes.
pub type VaultStateArc = Arc<VaultState>;
pub type ConfigStateArc = Arc<ConfigState>;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::State;
use tracing::{error, info, instrument, warn};

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
pub type SessionManagerState = Arc<SessionManager>;
pub type EventBusState = Arc<EventBus>;

// ===== Initialisation (appelée depuis setup dans lib.rs) =====

/// Enregistre les providers : Ollama toujours + providers API si une clé
/// est dispo (env var prioritaire, sinon config fichier).
pub fn init_provider_registry(config: &ConfigState) -> ProviderRegistryState {
    let registry = Arc::new(ProviderRegistry::new());
    let cfg = config.read();

    // Runtimes locaux : adresse issue de la config, sinon de l'environnement,
    // sinon du port conventionnel. La détection complète (processus, ports
    // exotiques) est déclenchée par `runtimes_detect` — la faire ici
    // retarderait l'ouverture de la fenêtre du temps des sondes réseau.
    for provider in local_providers_from_config(&cfg) {
        tauri::async_runtime::block_on(registry.register(provider));
    }

    // Providers API : clé env var OU config
    let register_api = |provider_id: &str, key: Option<String>| {
        if let Some(key) = key {
            let provider: Option<Arc<dyn crate::llm::provider::LlmProvider>> = match provider_id {
                "openai" => Some(Arc::new(crate::llm::openai::OpenAIProvider::new(key))),
                "anthropic" => Some(Arc::new(crate::llm::anthropic::AnthropicProvider::new(key))),
                "openrouter" => Some(Arc::new(crate::llm::openrouter::OpenRouterProvider::new(
                    key,
                ))),
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

/// Providers locaux déduits de la seule config (aucun accès réseau).
///
/// Ollama est toujours enregistré : c'est le runtime local par défaut, et un
/// provider présent mais injoignable donne à l'interface un message de santé
/// exploitable, là où un provider absent ne dit rien. llama.cpp n'apparaît
/// que si l'utilisateur a explicitement donné une adresse — sinon il faut
/// attendre la détection pour savoir s'il tourne.
fn local_providers_from_config(cfg: &AppConfig) -> Vec<Arc<dyn LlmProvider>> {
    let mut out: Vec<Arc<dyn LlmProvider>> = Vec::new();
    let ollama_host = cfg
        .ollama_host
        .clone()
        .or_else(|| std::env::var("OLLAMA_HOST").ok())
        .unwrap_or_default();
    out.push(Arc::new(crate::llm::ollama::OllamaProvider::new(
        ollama_host,
    )));

    if let Some(host) = cfg.llamacpp_host.as_ref().filter(|h| !h.trim().is_empty()) {
        out.push(Arc::new(crate::llm::llamacpp::LlamaCppProvider::new(
            host.clone(),
            cfg.api_key_for("llamacpp"),
        )));
    }
    out
}

/// Providers locaux déduits d'un scan des runtimes.
///
/// On retient la **première adresse joignable** de chaque famille : la liste
/// est déjà triée par priorité d'origine (config, puis environnement, puis
/// processus détecté, puis port conventionnel). Si aucune adresse ne répond,
/// on retombe sur la config pour qu'Ollama reste visible dans l'interface
/// avec son erreur de santé.
fn local_providers_from_scan(scan: &RuntimeScan, cfg: &AppConfig) -> Vec<Arc<dyn LlmProvider>> {
    let mut out: Vec<Arc<dyn LlmProvider>> = Vec::new();

    match scan.first_reachable(RuntimeKind::Ollama) {
        Some(rt) => out.push(Arc::new(crate::llm::ollama::OllamaProvider::new(
            rt.base_url.clone(),
        ))),
        None => out.push(Arc::new(crate::llm::ollama::OllamaProvider::new(
            cfg.ollama_host.clone().unwrap_or_default(),
        ))),
    }

    // llama.cpp : enregistré s'il répond, ou si l'utilisateur l'a configuré.
    let llamacpp_url = scan
        .first_reachable(RuntimeKind::LlamaCpp)
        .map(|rt| rt.base_url.clone())
        .or_else(|| cfg.llamacpp_host.clone().filter(|h| !h.trim().is_empty()));
    if let Some(url) = llamacpp_url {
        out.push(Arc::new(crate::llm::llamacpp::LlamaCppProvider::new(
            url,
            cfg.api_key_for("llamacpp"),
        )));
    }
    out
}

/// Registry des modèles locaux (GGUF/safetensors).
pub fn init_model_registry() -> ModelRegistryState {
    let mut dirs = Vec::new();
    if let Ok(p) = std::env::var("OBSIA_MODEL_DIR") {
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
#[instrument(skip(pool))]
pub async fn chat_send(
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
#[instrument(skip(bus, pool))]
pub async fn chat_stream(
    bus: State<'_, EventBusState>,
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
        bus.emit("llm:error", e.to_string());
        e.to_string()
    })?;

    // Chat sans session : pas d'annulation ciblée, d'où un drapeau inerte.
    let inerte = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (texte, _, echec) = consommer_flux(stream, &inerte, |token| {
        bus.emit("llm:token", token);
    })
    .await;
    if let Some(e) = echec {
        bus.emit("llm:error", e.clone());
        return Err(e);
    }
    bus.emit(
        "llm:done",
        ChatResponse {
            id: uuid::Uuid::new_v4().to_string(),
            model: payload.model.clone(),
            choices: vec![crate::llm::provider::ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".into(),
                    content: texte,
                    name: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: None,
        },
    );
    Ok(())
}

fn validate_payload(payload: &ChatRequestPayload) -> Result<(), String> {
    if payload.model.trim().is_empty() {
        return Err("model requis".into());
    }
    if payload.messages.is_empty() {
        return Err("messages ne doit pas être vide".into());
    }
    if payload.messages.iter().any(|m| m.content.trim().is_empty()) {
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
            name: p.name().to_string(),
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

// ===== Commandes — Runtimes locaux =====

/// Détecte les moteurs LLM locaux (Ollama, llama.cpp) et **recâble** les
/// providers en conséquence.
///
/// C'est l'action derrière le bouton « Détecter » de l'interface : elle sonde
/// les adresses candidates, remplace les providers locaux par ceux qui
/// répondent réellement, resynchronise le pool de repli, puis renvoie le
/// détail du scan (adresse retenue, origine, modèles, binaires installés).
#[tauri::command]
#[instrument(skip(config, registry, pool))]
pub async fn runtimes_detect(
    config: State<'_, ConfigStateArc>,
    registry: State<'_, ProviderRegistryState>,
    pool: State<'_, ProviderPoolState>,
) -> Result<RuntimeScan, String> {
    let cfg = config.read();
    let mut hosts = Vec::new();
    if let Some(h) = cfg.ollama_host.as_ref().filter(|h| !h.trim().is_empty()) {
        hosts.push((RuntimeKind::Ollama, h.clone()));
    }
    if let Some(h) = cfg.llamacpp_host.as_ref().filter(|h| !h.trim().is_empty()) {
        hosts.push((RuntimeKind::LlamaCpp, h.clone()));
    }

    let scan = discovery::scan(&hosts).await;

    // Recâblage : les providers locaux sont remplacés (pas ajoutés), et celui
    // qui a disparu du scan est retiré du registry.
    let locals = local_providers_from_scan(&scan, &cfg);
    let kept: Vec<String> = locals.iter().map(|p| p.id().to_string()).collect();
    for provider in locals {
        registry.register(provider).await;
    }
    for id in ["ollama", "llamacpp"] {
        if !kept.iter().any(|k| k == id) {
            registry.unregister(id).await;
        }
    }
    pool.replace_all(registry.list_all().await).await;

    info!(
        joignables = scan.runtimes.iter().filter(|r| r.reachable).count(),
        providers = kept.len(),
        "runtimes redétectés et providers recâblés"
    );
    Ok(scan)
}

// ===== Commandes — Config =====

/// Retourne la config SANS les valeurs de clés API (présence seulement).
#[tauri::command]
#[instrument(skip(config))]
pub fn config_get(config: State<'_, ConfigStateArc>) -> Result<ConfigView, String> {
    Ok(config.read().view())
}

/// Applique un patch de config (clés API, chemin coffre, provider défaut).
#[tauri::command]
#[instrument(skip(config))]
pub fn config_set(
    config: State<'_, ConfigStateArc>,
    patch: ConfigPatch,
) -> Result<ConfigView, String> {
    config.update(patch)
}

// ===== Commandes — Coffre (sandbox obsia_vault) =====

/// Liste les notes markdown du coffre.
#[tauri::command]
#[instrument(skip(vault))]
pub fn vault_list(vault: State<'_, VaultStateArc>) -> Result<Vec<VaultEntry>, String> {
    vault.list_notes()
}

/// Lit une note markdown (chemin relatif, extension .md).
#[tauri::command]
#[instrument(skip(vault))]
pub fn vault_read(vault: State<'_, VaultStateArc>, rel_path: String) -> Result<String, String> {
    vault.read_note(&rel_path)
}

/// Écrit une note markdown (chemin relatif, sandbox + protection).
#[tauri::command]
#[instrument(skip(vault))]
pub fn vault_write(
    vault: State<'_, VaultStateArc>,
    rel_path: String,
    content: String,
) -> Result<VaultEntry, String> {
    vault.write_note(&rel_path, &content)
}

/// Chemin absolu de la racine du coffre (pour l'UI).
#[tauri::command]
#[instrument(skip(vault))]
pub fn vault_path(vault: State<'_, VaultStateArc>) -> Result<String, String> {
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
            tx.send(TokenEvent::Done(self_echo_response(&req)))
                .await
                .unwrap();
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
                ChatMessage {
                    role: "user".into(),
                    content: "a".into(),
                    name: None,
                },
                ChatMessage {
                    role: "user".into(),
                    content: "b".into(),
                    name: None,
                },
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
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "x".into(),
                name: None,
            }],
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

// ===== Agents =====

/// Liste les agents du coffre (frontmatter validé par le backend).
#[tauri::command]
#[instrument(skip(vault))]
pub fn agents_list(vault: State<'_, VaultStateArc>) -> Result<Vec<AgentInfo>, String> {
    vault.agents_list()
}

/// Lit un agent complet (frontmatter + corps). `path` = `IA/agents/x.md`
/// ou simple nom `x.md`. Confiné à `IA/agents/` (sandbox + validation).
#[tauri::command]
#[instrument(skip(vault))]
pub fn agent_read(vault: State<'_, VaultStateArc>, path: String) -> Result<AgentDoc, String> {
    vault.agent_read(&path)
}

// ===== Commandes — Sessions =====

/// Paramètres d'ouverture d'une session.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCreatePayload {
    /// Agent du coffre pilotant la session (`IA/agents/<nom>.md`).
    #[serde(default)]
    pub agent: Option<String>,
    /// Équipe pilotant la session, exclusive de `agent`.
    #[serde(default)]
    pub team: Option<String>,
    /// Fournisseur ciblé. `None` = repli automatique sur le pool.
    #[serde(default)]
    pub provider: Option<String>,
    pub model: String,
}

/// Fragment de réponse. Porte `sessionId` : l'interface s'abonne une seule
/// fois et route vers le bon onglet.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokenEvent {
    pub session_id: String,
    pub token: String,
}

/// Fin de tour : message complet tel qu'il a été persisté.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDoneEvent {
    pub session_id: String,
    pub message: SessionMessage,
    pub meta: SessionMeta,
    /// Vrai si le tour a été interrompu par `session_cancel`.
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionErrorEvent {
    pub session_id: String,
    pub error: String,
}

/// Gestionnaire de sessions : stockage dans les données applicatives.
pub fn init_session_manager(app: &tauri::App) -> SessionManagerState {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("sessions");
    Arc::new(SessionManager::new(dir))
}

/// Liste les sessions, la plus récemment active en tête.
#[tauri::command]
#[instrument(skip(sessions))]
pub fn sessions_list(sessions: State<'_, SessionManagerState>) -> Result<Vec<SessionMeta>, String> {
    Ok(sessions.list())
}

/// Ouvre une session.
#[tauri::command]
#[instrument(skip(sessions))]
pub async fn session_create(
    sessions: State<'_, SessionManagerState>,
    payload: SessionCreatePayload,
) -> Result<SessionMeta, String> {
    if payload.model.trim().is_empty() {
        return Err("model requis".into());
    }
    if payload.agent.is_some() && payload.team.is_some() {
        return Err("une session est menée par un agent OU par une équipe".into());
    }
    sessions
        .create(payload.agent, payload.team, payload.provider, payload.model)
        .await
}

/// Session complète, historique compris.
#[tauri::command]
#[instrument(skip(sessions))]
pub fn session_get(
    sessions: State<'_, SessionManagerState>,
    session_id: String,
) -> Result<Session, String> {
    sessions.get(&session_id)
}

#[tauri::command]
#[instrument(skip(sessions))]
pub async fn session_rename(
    sessions: State<'_, SessionManagerState>,
    session_id: String,
    title: String,
) -> Result<SessionMeta, String> {
    sessions.rename(&session_id, &title).await
}

#[tauri::command]
#[instrument(skip(sessions))]
pub async fn session_delete(
    sessions: State<'_, SessionManagerState>,
    session_id: String,
) -> Result<(), String> {
    sessions.delete(&session_id).await
}

/// Interrompt le tour en cours. Le texte déjà produit est conservé.
#[tauri::command]
#[instrument(skip(sessions))]
pub async fn session_cancel(
    sessions: State<'_, SessionManagerState>,
    session_id: String,
) -> Result<bool, String> {
    Ok(sessions.cancel(&session_id).await)
}

/// Changement d'orateur dans une session d'équipe : l'interface ouvre une
/// nouvelle bulle au nom de l'agent qui prend la parole.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTurnEvent {
    pub session_id: String,
    pub agent: String,
    pub turn: u32,
}

/// Message ajouté au fil en cours d'exécution.
///
/// Une exécution d'équipe enchaîne plusieurs tours de parole : sans cet
/// événement, les interventions intermédiaires n'apparaîtraient qu'à la fin
/// de toute l'exécution.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessageEvent {
    pub session_id: String,
    pub message: SessionMessage,
    pub meta: SessionMeta,
}

/// Consomme un flux de jetons, chaque fragment étant remis à `emit`.
///
/// Renvoie `(texte accumulé, interrompu, erreur éventuelle)`. Partagé par le
/// chat à un agent, l'orchestration d'équipe et l'exécution de plans — une
/// seule implémentation de l'annulation, du repli sur la réponse finale et de
/// la fermeture du flux. Seule la destination des jetons change, d'où le
/// paramètre `emit` plutôt qu'un nom d'événement figé.
async fn consommer_flux<F: Fn(String)>(
    mut stream: crate::llm::provider::TokenStream,
    annule: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    emit: F,
) -> (String, bool, Option<String>) {
    let mut texte = String::new();
    let mut cancelled = false;
    let mut echec = None;

    while let Some(event) = stream.recv().await {
        if annule.load(std::sync::atomic::Ordering::SeqCst) {
            cancelled = true;
            break;
        }
        match event {
            TokenEvent::Token(token) => {
                texte.push_str(&token);
                emit(token);
            }
            TokenEvent::Done(response) => {
                // La réponse finale fait foi : elle porte le texte complet, y
                // compris ce qu'un fournisseur non streamant n'aurait pas émis
                // jeton par jeton.
                if let Some(choix) = response.choices.first() {
                    if !choix.message.content.is_empty() {
                        texte = choix.message.content.clone();
                    }
                }
                break;
            }
            TokenEvent::Error(e) => {
                echec = Some(e);
                break;
            }
        }
    }

    // Fermer le flux fait échouer l'envoi côté fournisseur, ce qui arrête la
    // génération au lieu de la laisser tourner dans le vide.
    drop(stream);
    (texte, cancelled, echec)
}

/// Envoie un message dans une session et diffuse la réponse.
///
/// Le prompt système est relu **dans le coffre à chaque tour** : modifier un
/// agent doit prendre effet immédiatement, sans rouvrir la session.
#[tauri::command]
#[instrument(skip(bus, sessions, pool, vault))]
pub async fn session_send(
    bus: State<'_, EventBusState>,
    sessions: State<'_, SessionManagerState>,
    pool: State<'_, ProviderPoolState>,
    vault: State<'_, VaultStateArc>,
    session_id: String,
    content: String,
) -> Result<(), String> {
    session_send_impl(
        bus.inner(),
        sessions.inner(),
        pool.inner(),
        vault.inner(),
        session_id,
        content,
        None,
    )
    .await
    .map(|_| ())
}

/// Corps de [`session_send`], indépendant de Tauri.
///
/// Le serveur distant appelle exactement cette fonction : un client attaché à
/// distance exécute le même code que la fenêtre locale, sans chemin parallèle
/// à maintenir en double.
pub async fn session_send_impl(
    bus: &EventBus,
    sessions: &SessionManager,
    pool: &ProviderPool,
    vault: &VaultState,
    session_id: String,
    content: String,
    // `system_override` impose le prompt système et court-circuite l'agent du
    // coffre : c'est ainsi que l'intendant, qui n'en est pas un, s'insère.
    system_override: Option<&str>,
) -> Result<String, String> {
    if content.trim().is_empty() {
        return Err("message vide refusé".into());
    }

    // L'agent est optionnel, et un agent illisible ne doit pas bloquer la
    // conversation : on continue sans prompt système, en le signalant.
    let session = sessions.get(&session_id)?;
    let system_prompt = match system_override {
        // L'intendant n'est pas un agent du coffre : son prompt est fourni
        // par l'appelant plutôt que lu dans `IA/agents/`.
        Some(p) => Some(p.to_string()),
        None => session.meta.agent.as_ref().and_then(|nom| {
            match vault.agent_read(&format!("{nom}.md")) {
                Ok(doc) => Some(doc.content),
                Err(e) => {
                    warn!(agent = %nom, %e, "prompt système introuvable, session sans agent");
                    None
                }
            }
        }),
    };

    sessions
        .push_message(&session_id, SessionMessage::new("user", content))
        .await?;
    let session = sessions.get(&session_id)?;

    let req = ChatRequest {
        model: session.meta.model.clone(),
        messages: session.to_chat_messages(system_prompt.as_deref()),
        temperature: None,
        max_tokens: None,
        stream: true,
        metadata: HashMap::new(),
    };

    let annule = sessions.begin(&session_id).await;
    let ouverture = match &session.meta.provider {
        Some(pid) => pool.chat_stream_with(pid, req).await,
        None => pool.chat_stream_with_fallback(req).await,
    };
    let stream = match ouverture {
        Ok(s) => s,
        Err(e) => {
            sessions.finish(&session_id).await;
            bus.emit(
                "session:error",
                SessionErrorEvent {
                    session_id: session_id.clone(),
                    error: e.to_string(),
                },
            );
            error!(%e, "ouverture du flux de session échouée");
            return Err(e.to_string());
        }
    };

    let (texte, cancelled, echec) = consommer_flux(stream, &annule, |token| {
        bus.emit(
            "session:token",
            SessionTokenEvent {
                session_id: session_id.clone(),
                token,
            },
        );
    })
    .await;
    sessions.finish(&session_id).await;

    if let Some(e) = echec {
        bus.emit(
            "session:error",
            SessionErrorEvent {
                session_id: session_id.clone(),
                error: e.clone(),
            },
        );
        return Err(e);
    }

    // Un tour annulé sans le moindre jeton n'a rien à conserver.
    if texte.is_empty() && cancelled {
        let meta = sessions.get(&session_id)?.meta;
        bus.emit(
            "session:done",
            SessionDoneEvent {
                session_id: session_id.clone(),
                message: SessionMessage::new("assistant", String::new()),
                meta,
                cancelled: true,
            },
        );
        return Ok(String::new());
    }

    let message =
        SessionMessage::new("assistant", texte.clone()).with_agent(session.meta.agent.clone());
    let meta = sessions.push_message(&session_id, message.clone()).await?;
    bus.emit(
        "session:done",
        SessionDoneEvent {
            session_id,
            message,
            meta,
            cancelled,
        },
    );
    // Le texte produit est rendu à l'appelant : l'intendant y cherche son
    // bloc d'actions, ce que la fenêtre n'a pas à faire.
    Ok(texte)
}

/// Exporte une session en note Markdown dans `brouillon/`.
///
/// L'écriture vise la seule zone écrivable du coffre et reproduit la structure
/// de `mémoire/` : la revue humaine n'a plus qu'à déplacer l'arborescence
/// (cf. `VAULT-CONTRACT.md` §2).
#[tauri::command]
#[instrument(skip(sessions, vault))]
pub fn session_export(
    sessions: State<'_, SessionManagerState>,
    vault: State<'_, VaultStateArc>,
    session_id: String,
    project: String,
) -> Result<VaultEntry, String> {
    if project.trim().is_empty() {
        return Err("nom de projet requis".into());
    }
    let session = sessions.get(&session_id)?;
    if session.messages.is_empty() {
        return Err("session vide : rien à exporter".into());
    }
    let chemin = crate::session::export_path(&session, &project);
    let entry = vault.write_note(&chemin, &crate::session::to_markdown(&session))?;
    info!(note = %entry.path, "session exportée dans le brouillon du coffre");
    Ok(entry)
}

// ===== Commandes — Équipes =====

pub type TeamStoreState = Arc<TeamStore>;

/// Création ou mise à jour d'une équipe.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamSavePayload {
    /// `None` = création.
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub members: Vec<TeamMember>,
    pub strategy: TeamStrategy,
    pub max_turns: u32,
}

pub fn init_team_store(app: &tauri::App) -> TeamStoreState {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("teams");
    Arc::new(TeamStore::new(dir))
}

#[tauri::command]
#[instrument(skip(teams))]
pub fn teams_list(teams: State<'_, TeamStoreState>) -> Result<Vec<Team>, String> {
    Ok(teams.list())
}

/// Enregistre une équipe. La validation (membres uniques, budget borné,
/// superviseur accompagné) a lieu ici : une exécution ne doit jamais échouer
/// à mi-parcours sur une équipe mal formée, budget déjà consommé.
#[tauri::command]
#[instrument(skip(teams, vault))]
pub fn team_save(
    teams: State<'_, TeamStoreState>,
    vault: State<'_, VaultStateArc>,
    payload: TeamSavePayload,
) -> Result<Team, String> {
    let mut team = match &payload.id {
        Some(id) => teams.load(id)?,
        None => Team::new(
            payload.name.clone(),
            payload.description.clone(),
            payload.members.clone(),
            payload.strategy,
            payload.max_turns,
        ),
    };
    team.name = payload.name;
    team.description = payload.description;
    team.members = payload.members;
    team.strategy = payload.strategy;
    team.max_turns = payload.max_turns;
    team.updated_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    team.validate()?;

    // Un membre inconnu du coffre produirait une équipe qui tourne sans
    // prompt système : autant le refuser tout de suite.
    let connus: Vec<String> = vault
        .agents_list()
        .unwrap_or_default()
        .into_iter()
        .map(|a| a.name)
        .collect();
    if !connus.is_empty() {
        if let Some(m) = team.members.iter().find(|m| !connus.contains(&m.agent)) {
            return Err(format!(
                "l'agent « {} » n'existe pas dans le coffre (agents connus : {})",
                m.agent,
                connus.join(", ")
            ));
        }
    }

    teams.save(&team)?;
    info!(equipe = %team.name, membres = team.members.len(), "équipe enregistrée");
    Ok(team)
}

#[tauri::command]
#[instrument(skip(teams))]
pub fn team_delete(teams: State<'_, TeamStoreState>, team_id: String) -> Result<(), String> {
    teams.delete(&team_id)
}

/// Fait travailler l'équipe d'une session sur un objectif.
///
/// Chaque membre parle à son tour avec **son propre modèle**, voit tout le fil
/// (chaque message étant attribué à son auteur), et écrit sa réponse dans la
/// session. L'exécution s'arrête au premier des trois : marqueur de fin,
/// budget de tours épuisé, ou annulation.
#[tauri::command]
#[instrument(skip(bus, sessions, teams, pool, vault))]
pub async fn team_run(
    bus: State<'_, EventBusState>,
    sessions: State<'_, SessionManagerState>,
    teams: State<'_, TeamStoreState>,
    pool: State<'_, ProviderPoolState>,
    vault: State<'_, VaultStateArc>,
    session_id: String,
    objective: String,
) -> Result<(), String> {
    team_run_impl(
        bus.inner(),
        sessions.inner(),
        teams.inner(),
        pool.inner(),
        vault.inner(),
        session_id,
        objective,
    )
    .await
}

/// Corps de [`team_run`], indépendant de Tauri.
pub async fn team_run_impl(
    bus: &EventBus,
    sessions: &SessionManager,
    teams: &TeamStore,
    pool: &ProviderPool,
    vault: &VaultState,
    session_id: String,
    objective: String,
) -> Result<(), String> {
    let objectif = objective.trim().to_string();
    if objectif.is_empty() {
        return Err("objectif requis".into());
    }
    let session = sessions.get(&session_id)?;
    let team_id = session
        .meta
        .team
        .clone()
        .ok_or("cette session n'est pas pilotée par une équipe")?;
    let team = teams.load(&team_id)?;
    team.validate()?;

    sessions
        .push_message(&session_id, SessionMessage::new("user", objectif.clone()))
        .await?;

    let annule = sessions.begin(&session_id).await;
    let mut designe: Option<String> = None;
    let mut tour: u32 = 0;
    let mut cancelled = false;

    while let Some(membre) = team::prochain_intervenant(&team, tour, designe.as_deref()) {
        if annule.load(std::sync::atomic::Ordering::SeqCst) {
            cancelled = true;
            break;
        }
        bus.emit(
            "session:turn",
            SessionTurnEvent {
                session_id: session_id.clone(),
                agent: membre.agent.clone(),
                turn: tour,
            },
        );

        // Le prompt de l'agent vient du coffre ; le briefing d'équipe s'y
        // ajoute sans le remplacer — l'agent reste lui-même.
        let corps = match vault.agent_read(&format!("{}.md", membre.agent)) {
            Ok(doc) => doc.content,
            Err(e) => {
                warn!(agent = %membre.agent, %e, "agent illisible, briefing seul");
                String::new()
            }
        };
        let systeme = format!("{corps}\n\n{}", team::briefing(&team, &membre, &objectif));

        // Relu à chaque tour : le membre doit voir ce que les précédents ont dit.
        let courante = sessions.get(&session_id)?;
        let req = ChatRequest {
            model: membre.model.clone(),
            messages: courante.to_chat_messages(Some(&systeme)),
            temperature: None,
            max_tokens: None,
            stream: true,
            metadata: HashMap::new(),
        };

        let ouverture = match &membre.provider {
            Some(pid) => pool.chat_stream_with(pid, req).await,
            None => pool.chat_stream_with_fallback(req).await,
        };
        let stream = match ouverture {
            Ok(s) => s,
            Err(e) => {
                sessions.finish(&session_id).await;
                bus.emit(
                    "session:error",
                    SessionErrorEvent {
                        session_id: session_id.clone(),
                        error: format!("{} : {e}", membre.agent),
                    },
                );
                return Err(e.to_string());
            }
        };

        let (texte, stoppe, echec) = consommer_flux(stream, &annule, |token| {
            bus.emit(
                "session:token",
                SessionTokenEvent {
                    session_id: session_id.clone(),
                    token,
                },
            );
        })
        .await;
        if let Some(e) = echec {
            sessions.finish(&session_id).await;
            bus.emit(
                "session:error",
                SessionErrorEvent {
                    session_id: session_id.clone(),
                    error: format!("{} : {e}", membre.agent),
                },
            );
            return Err(e);
        }
        if !texte.trim().is_empty() {
            let message = SessionMessage::new("assistant", texte.clone())
                .with_agent(Some(membre.agent.clone()));
            let meta = sessions.push_message(&session_id, message.clone()).await?;
            bus.emit(
                "session:message",
                SessionMessageEvent {
                    session_id: session_id.clone(),
                    message,
                    meta,
                },
            );
        }
        if stoppe {
            cancelled = true;
            break;
        }
        if team::est_termine(&texte) {
            info!(equipe = %team.name, tours = tour + 1, "objectif déclaré atteint");
            break;
        }
        // Seul le superviseur distribue la parole.
        if team.supervisor().is_some_and(|s| s.agent == membre.agent) {
            designe = team::designation(&team, &texte);
        }
        tour += 1;
    }

    sessions.finish(&session_id).await;
    let session = sessions.get(&session_id)?;
    let dernier = session
        .messages
        .last()
        .cloned()
        .unwrap_or_else(|| SessionMessage::new("assistant", String::new()));
    bus.emit(
        "session:done",
        SessionDoneEvent {
            session_id,
            message: dernier,
            meta: session.meta,
            cancelled,
        },
    );
    Ok(())
}

// ===== Commandes — Plans =====

pub type PlanManagerState = Arc<PlanManager>;

/// Création ou mise à jour d'un plan.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanSavePayload {
    /// `None` = création.
    #[serde(default)]
    pub id: Option<String>,
    pub title: String,
    pub objective: String,
    pub steps: Vec<PlanStep>,
}

/// Demande de décomposition d'un objectif par un modèle.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanDraftPayload {
    pub objective: String,
    /// Agent affecté aux étapes dont l'agent proposé est inconnu.
    pub agent: String,
    #[serde(default)]
    pub provider: Option<String>,
    pub model: String,
}

/// Fragment produit par une étape en cours.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanTokenEvent {
    pub plan_id: String,
    pub step_id: String,
    pub token: String,
}

/// Nouvel état du plan après chaque vague d'étapes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanUpdateEvent {
    pub plan: Plan,
}

pub fn init_plan_manager(app: &tauri::App) -> PlanManagerState {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("plans");
    Arc::new(PlanManager::new(dir))
}

#[tauri::command]
#[instrument(skip(plans))]
pub fn plans_list(plans: State<'_, PlanManagerState>) -> Result<Vec<Plan>, String> {
    Ok(plans.list())
}

#[tauri::command]
#[instrument(skip(plans))]
pub async fn plan_save(
    plans: State<'_, PlanManagerState>,
    payload: PlanSavePayload,
) -> Result<Plan, String> {
    let mut plan = match &payload.id {
        Some(id) => plans.load(id)?,
        None => Plan::new(payload.title.clone(), payload.objective.clone(), Vec::new()),
    };
    plan.title = payload.title;
    plan.objective = payload.objective;
    plan.steps = payload.steps;
    plan.refresh_status();
    plan.validate()?;
    plans.save(&plan).await?;
    Ok(plan)
}

#[tauri::command]
#[instrument(skip(plans))]
pub async fn plan_delete(
    plans: State<'_, PlanManagerState>,
    plan_id: String,
) -> Result<(), String> {
    plans.delete(&plan_id).await
}

#[tauri::command]
#[instrument(skip(plans))]
pub async fn plan_cancel(
    plans: State<'_, PlanManagerState>,
    plan_id: String,
) -> Result<bool, String> {
    Ok(plans.cancel(&plan_id).await)
}

/// Fait décomposer un objectif en étapes par un modèle.
///
/// Le plan renvoyé n'est **pas** enregistré : il est proposé à la revue.
/// Un découpage produit par un modèle mérite d'être relu avant d'engager le
/// budget de son exécution.
#[tauri::command]
#[instrument(skip(pool, vault))]
pub async fn plan_draft(
    pool: State<'_, ProviderPoolState>,
    vault: State<'_, VaultStateArc>,
    payload: PlanDraftPayload,
) -> Result<Plan, String> {
    let objectif = payload.objective.trim().to_string();
    if objectif.is_empty() {
        return Err("objectif requis".into());
    }
    let agents: Vec<String> = vault
        .agents_list()
        .unwrap_or_default()
        .into_iter()
        .map(|a| a.name)
        .collect();

    let req = ChatRequest {
        model: payload.model.clone(),
        messages: vec![ChatMessage {
            role: "user".into(),
            content: plan::prompt_decomposition(&objectif, &agents),
            name: None,
        }],
        temperature: Some(0.2),
        max_tokens: None,
        stream: false,
        metadata: HashMap::new(),
    };
    let reponse = match &payload.provider {
        Some(pid) => pool.chat_with(pid, req).await,
        None => pool.chat_with_fallback(req).await,
    }
    .map_err(|e| e.to_string())?;

    let texte = reponse
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default();
    let json =
        plan::extraire_json(&texte).ok_or("le modèle n'a pas produit de JSON exploitable")?;
    let ebauche: plan::Ebauche =
        serde_json::from_str(json).map_err(|e| format!("découpage illisible : {e}"))?;

    plan::plan_depuis_ebauche(
        ebauche,
        &objectif,
        &agents,
        &payload.agent,
        payload.provider,
        &payload.model,
    )
}

/// Exécute un plan.
///
/// Les étapes dont les dépendances sont satisfaites partent **ensemble** :
/// deux branches indépendantes n'ont aucune raison de s'attendre. Chaque
/// étape ne reçoit que l'objectif et le résultat de ses dépendances, ce qui
/// garde les prompts courts et évite qu'une branche en pollue une autre.
///
/// Les étapes déjà abouties ne sont pas refaites : relancer un plan interrompu
/// reprend là où il s'était arrêté.
#[tauri::command]
#[instrument(skip(bus, plans, pool, vault))]
pub async fn plan_run(
    bus: State<'_, EventBusState>,
    plans: State<'_, PlanManagerState>,
    pool: State<'_, ProviderPoolState>,
    vault: State<'_, VaultStateArc>,
    plan_id: String,
) -> Result<Plan, String> {
    plan_run_impl(
        bus.inner(),
        plans.inner(),
        pool.inner(),
        vault.inner(),
        plan_id,
    )
    .await
}

/// Corps de [`plan_run`], indépendant de Tauri.
pub async fn plan_run_impl(
    bus: &EventBus,
    plans: &PlanManager,
    pool: &ProviderPool,
    vault: &VaultState,
    plan_id: String,
) -> Result<Plan, String> {
    let mut plan = plans.load(&plan_id)?;
    plan.validate()?;
    plan.reset_unfinished();
    plan.refresh_status();

    let annule = plans.begin(&plan_id).await;
    let mut interrompu = false;

    loop {
        if annule.load(std::sync::atomic::Ordering::SeqCst) {
            interrompu = true;
            break;
        }
        // Les contextes sont calculés maintenant : les étapes de la vague
        // s'exécutent en parallèle et ne doivent pas emprunter le plan.
        let vague: Vec<(PlanStep, String)> = plan
            .ready_steps()
            .into_iter()
            .map(|s| (s.clone(), plan.context_for(s)))
            .collect();
        if vague.is_empty() {
            break;
        }
        for (etape, _) in &vague {
            plan.mark_running(&etape.id);
        }
        plans.save(&plan).await?;
        bus.emit("plan:update", PlanUpdateEvent { plan: plan.clone() });

        let taches = vague.into_iter().map(|(etape, contexte)| {
            let annule = &annule;
            let plan_id = plan_id.clone();
            async move {
                let resultat =
                    executer_etape(bus, pool, vault, &plan_id, &etape, &contexte, annule).await;
                (etape.id.clone(), resultat)
            }
        });
        let resultats = futures::future::join_all(taches).await;

        for (id, resultat) in resultats {
            match resultat {
                Ok(Some(texte)) => plan.mark_done(&id, texte),
                // `None` = interrompu avant d'aboutir : l'étape est laissée
                // en attente pour être reprise telle quelle.
                Ok(None) => interrompu = true,
                Err(e) => {
                    warn!(etape = %id, %e, "étape en échec");
                    plan.mark_failed(&id, e);
                }
            }
        }
        plan.refresh_status();
        plans.save(&plan).await?;
        bus.emit("plan:update", PlanUpdateEvent { plan: plan.clone() });

        if interrompu {
            break;
        }
    }

    if interrompu {
        plan.reset_unfinished();
        plan.status = PlanStatus::Cancelled;
    } else {
        plan.refresh_status();
    }
    plans.save(&plan).await?;
    plans.finish(&plan_id).await;
    bus.emit("plan:update", PlanUpdateEvent { plan: plan.clone() });
    let (faites, total) = plan.progress();
    info!(plan = %plan.title, faites, total, "exécution du plan terminée");
    Ok(plan)
}

/// Exécute une étape. `Ok(None)` signale une interruption avant aboutissement.
async fn executer_etape(
    bus: &EventBus,
    pool: &ProviderPool,
    vault: &VaultState,
    plan_id: &str,
    etape: &PlanStep,
    contexte: &str,
    annule: &Arc<std::sync::atomic::AtomicBool>,
) -> Result<Option<String>, String> {
    let corps = match vault.agent_read(&format!("{}.md", etape.agent)) {
        Ok(doc) => doc.content,
        Err(e) => {
            warn!(agent = %etape.agent, %e, "agent illisible, étape sans prompt système");
            String::new()
        }
    };
    let req = ChatRequest {
        model: etape.model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".into(),
                content: corps,
                name: None,
            },
            ChatMessage {
                role: "user".into(),
                content: contexte.to_string(),
                name: None,
            },
        ],
        temperature: None,
        max_tokens: None,
        stream: true,
        metadata: HashMap::new(),
    };

    let stream = match &etape.provider {
        Some(pid) => pool.chat_stream_with(pid, req).await,
        None => pool.chat_stream_with_fallback(req).await,
    }
    .map_err(|e| e.to_string())?;

    let (texte, stoppe, echec) = consommer_flux(stream, annule, |token| {
        bus.emit(
            "plan:token",
            PlanTokenEvent {
                plan_id: plan_id.to_string(),
                step_id: etape.id.clone(),
                token,
            },
        );
    })
    .await;

    if let Some(e) = echec {
        return Err(e);
    }
    if stoppe {
        return Ok(None);
    }
    if texte.trim().is_empty() {
        return Err("l'étape n'a produit aucun résultat".into());
    }
    Ok(Some(texte))
}

// ===== Commandes — Serveur distant =====

pub type RemoteStateArc = Arc<RemoteState>;

/// État du daemon, tel que l'interface l'affiche.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStatus {
    pub running: bool,
    /// Adresse réellement écoutée, quand le serveur tourne.
    pub address: Option<String>,
    /// Adresse configurée pour le prochain démarrage.
    pub bind: String,
    /// Démarrage automatique au lancement.
    pub enabled: bool,
    pub token_configured: bool,
    /// Faux quand le serveur est joignable depuis le réseau.
    pub loopback_only: bool,
}

/// Assemble le harness partagé entre la fenêtre et le serveur distant.
#[allow(clippy::too_many_arguments)]
fn harness(
    bus: &EventBusState,
    sessions: &SessionManagerState,
    teams: &TeamStoreState,
    plans: &PlanManagerState,
    pool: &ProviderPoolState,
    registry: &ProviderRegistryState,
    vault: &VaultStateArc,
) -> Harness {
    Harness {
        bus: bus.clone(),
        sessions: sessions.clone(),
        teams: teams.clone(),
        plans: plans.clone(),
        pool: pool.clone(),
        registry: registry.clone(),
        vault: vault.clone(),
    }
}

fn bind_configure(cfg: &crate::config::AppConfig) -> String {
    cfg.remote_bind
        .clone()
        .filter(|b| !b.trim().is_empty())
        .unwrap_or_else(|| format!("127.0.0.1:{}", remote::PORT_DEFAUT))
}

#[tauri::command]
#[instrument(skip(config, remote_state))]
pub async fn remote_status(
    config: State<'_, ConfigStateArc>,
    remote_state: State<'_, RemoteStateArc>,
) -> Result<RemoteStatus, String> {
    let cfg = config.read();
    let addr = remote_state.adresse().await;
    Ok(RemoteStatus {
        running: addr.is_some(),
        address: addr.map(|a| a.to_string()),
        bind: bind_configure(&cfg),
        enabled: cfg.remote_enabled,
        token_configured: cfg.remote_token().is_some(),
        loopback_only: addr.map(|a| remote::est_boucle(&a)).unwrap_or(true),
    })
}

/// Démarre le serveur distant.
///
/// Un jeton est engendré à la volée s'il n'en existe pas : il n'y a pas de
/// mode « sans authentification », même en écoute locale.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
#[instrument(skip(
    config,
    remote_state,
    bus,
    sessions,
    teams,
    plans,
    pool,
    registry,
    vault
))]
pub async fn remote_start(
    config: State<'_, ConfigStateArc>,
    remote_state: State<'_, RemoteStateArc>,
    bus: State<'_, EventBusState>,
    sessions: State<'_, SessionManagerState>,
    teams: State<'_, TeamStoreState>,
    plans: State<'_, PlanManagerState>,
    pool: State<'_, ProviderPoolState>,
    registry: State<'_, ProviderRegistryState>,
    vault: State<'_, VaultStateArc>,
) -> Result<RemoteStatus, String> {
    let cfg = config.read();
    let jeton = match cfg.remote_token() {
        Some(j) => j,
        None => {
            let j = remote::generer_jeton();
            config.set_remote_token(j.clone())?;
            info!("jeton distant engendré");
            j
        }
    };
    let bind = bind_configure(&cfg);
    let h = harness(&bus, &sessions, &teams, &plans, &pool, &registry, &vault);
    remote_state.demarrer(h, &bind, &jeton).await?;
    config.update(crate::config::ConfigPatch {
        remote_enabled: Some(true),
        ..Default::default()
    })?;
    remote_status(config, remote_state).await
}

#[tauri::command]
#[instrument(skip(config, remote_state))]
pub async fn remote_stop(
    config: State<'_, ConfigStateArc>,
    remote_state: State<'_, RemoteStateArc>,
) -> Result<RemoteStatus, String> {
    remote_state.arreter().await;
    config.update(crate::config::ConfigPatch {
        remote_enabled: Some(false),
        ..Default::default()
    })?;
    remote_status(config, remote_state).await
}

/// Révèle le jeton, pour que l'utilisateur le recopie sur le poste client.
///
/// Commande **locale uniquement** : elle n'est pas dans la liste blanche du
/// serveur, un client déjà connecté ne peut donc pas relire le secret.
#[tauri::command]
#[instrument(skip(config))]
pub fn remote_token_read(config: State<'_, ConfigStateArc>) -> Result<String, String> {
    config
        .read()
        .remote_token()
        .ok_or_else(|| "aucun jeton engendré — démarrez le serveur une fois".to_string())
}

/// Engendre un nouveau jeton et invalide l'ancien.
///
/// Le serveur est redémarré s'il tournait : sans cela l'ancien jeton
/// resterait valide jusqu'au prochain lancement de l'application.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
#[instrument(skip(
    config,
    remote_state,
    bus,
    sessions,
    teams,
    plans,
    pool,
    registry,
    vault
))]
pub async fn remote_token_rotate(
    config: State<'_, ConfigStateArc>,
    remote_state: State<'_, RemoteStateArc>,
    bus: State<'_, EventBusState>,
    sessions: State<'_, SessionManagerState>,
    teams: State<'_, TeamStoreState>,
    plans: State<'_, PlanManagerState>,
    pool: State<'_, ProviderPoolState>,
    registry: State<'_, ProviderRegistryState>,
    vault: State<'_, VaultStateArc>,
) -> Result<String, String> {
    let jeton = remote::generer_jeton();
    config.set_remote_token(jeton.clone())?;
    if remote_state.adresse().await.is_some() {
        let bind = bind_configure(&config.read());
        let h = harness(&bus, &sessions, &teams, &plans, &pool, &registry, &vault);
        remote_state.demarrer(h, &bind, &jeton).await?;
    }
    info!("jeton distant renouvelé");
    Ok(jeton)
}

// ===== Commandes — Patches d'interface et plugins =====

pub type PluginStoreState = Arc<PluginStore>;

/// Création ou mise à jour d'un patch d'interface.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchSavePayload {
    /// `None` = création.
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub theme: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub layout: LayoutPatch,
}

pub fn init_plugin_store(app: &tauri::App) -> PluginStoreState {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let store = PluginStore::new(dir);
    // Le dossier est créé au démarrage pour que l'utilisateur puisse y
    // déposer un plugin sans avoir à deviner le chemin ni à le créer.
    if let Err(e) = std::fs::create_dir_all(store.plugins_dir()) {
        warn!(%e, "dossier des plugins non créé");
    }
    Arc::new(store)
}

#[tauri::command]
#[instrument(skip(plugins))]
pub fn patches_list(plugins: State<'_, PluginStoreState>) -> Result<Vec<UiPatch>, String> {
    Ok(plugins.list_patches())
}

/// Enregistre un patch. La validation (valeurs CSS, bornes de disposition)
/// a lieu ici : c'est le seul rempart, puisque le patch finit injecté dans
/// la feuille de style de la fenêtre.
#[tauri::command]
#[instrument(skip(plugins))]
pub fn patch_save(
    plugins: State<'_, PluginStoreState>,
    payload: PatchSavePayload,
) -> Result<UiPatch, String> {
    let mut patch = match &payload.id {
        Some(id) => plugins.load_patch(id)?,
        None => UiPatch::new(payload.name.clone(), payload.description.clone()),
    };
    patch.name = payload.name;
    patch.description = payload.description;
    patch.theme = payload.theme;
    patch.layout = payload.layout;
    patch.updated_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    plugins.save_patch(&patch)?;
    Ok(patch)
}

#[tauri::command]
#[instrument(skip(plugins))]
pub fn patch_delete(plugins: State<'_, PluginStoreState>, patch_id: String) -> Result<(), String> {
    plugins.delete_patch(&patch_id)
}

/// Active ou désactive un patch, et renvoie le CSS cumulé qui en résulte.
#[tauri::command]
#[instrument(skip(plugins))]
pub fn patch_toggle(
    plugins: State<'_, PluginStoreState>,
    patch_id: String,
    enabled: bool,
) -> Result<String, String> {
    let mut patch = plugins.load_patch(&patch_id)?;
    patch.enabled = enabled;
    plugins.save_patch(&patch)?;
    Ok(plugins.css_actif())
}

/// CSS cumulé des patches actifs, à poser sur `:root`.
#[tauri::command]
#[instrument(skip(plugins))]
pub fn patch_css(plugins: State<'_, PluginStoreState>) -> Result<String, String> {
    Ok(plugins.css_actif())
}

/// Plugin prêt à être chargé par l'interface.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedPlugin {
    #[serde(flatten)]
    pub plugin: InstalledPlugin,
    /// Code source du plugin.
    pub source: String,
    /// Commandes que ses permissions lui ouvrent.
    pub allowed_commands: Vec<String>,
}

#[tauri::command]
#[instrument(skip(plugins))]
pub fn plugins_list(plugins: State<'_, PluginStoreState>) -> Result<Vec<InstalledPlugin>, String> {
    Ok(plugins.list_plugins())
}

/// Dossier où déposer un plugin.
#[tauri::command]
#[instrument(skip(plugins))]
pub fn plugins_dir(plugins: State<'_, PluginStoreState>) -> Result<String, String> {
    Ok(plugins.plugins_dir().display().to_string())
}

/// Charge les plugins actifs, avec leur code et leurs commandes permises.
///
/// Un plugin dont le fichier a changé depuis l'activation n'est pas renvoyé :
/// il attend d'être réapprouvé.
#[tauri::command]
#[instrument(skip(plugins))]
pub fn plugins_load(plugins: State<'_, PluginStoreState>) -> Result<Vec<LoadedPlugin>, String> {
    let mut out = Vec::new();
    for p in plugins.list_plugins() {
        if !p.enabled || p.needs_review {
            continue;
        }
        match plugins.source(&p.manifest.id) {
            Ok(source) => {
                let allowed_commands = plugin::commandes_autorisees(&p.manifest.permissions);
                out.push(LoadedPlugin {
                    plugin: p,
                    source,
                    allowed_commands,
                });
            }
            Err(e) => warn!(plugin = %p.manifest.id, %e, "code illisible, plugin ignoré"),
        }
    }
    info!(actifs = out.len(), "plugins chargés");
    Ok(out)
}

/// Active un plugin en approuvant le code présent sur disque.
#[tauri::command]
#[instrument(skip(plugins))]
pub fn plugin_enable(
    plugins: State<'_, PluginStoreState>,
    plugin_id: String,
) -> Result<InstalledPlugin, String> {
    plugins.enable(&plugin_id)
}

#[tauri::command]
#[instrument(skip(plugins))]
pub fn plugin_disable(
    plugins: State<'_, PluginStoreState>,
    plugin_id: String,
) -> Result<(), String> {
    plugins.disable(&plugin_id)
}

// ===== Commandes — Outils MCP =====

/// Liste les outils MCP déclarés dans le coffre, avec les agents qui les
/// utilisent.
#[tauri::command]
#[instrument(skip(vault))]
pub fn mcp_list(vault: State<'_, VaultStateArc>) -> Result<Vec<McpInfo>, String> {
    vault.mcp_list()
}

/// Rédige une déclaration MCP dans `brouillon/` et renvoie son chemin.
///
/// Jamais directement dans `IA/MCP/` : donner à des agents un outil que
/// personne n'a relu reviendrait à leur ouvrir un accès sans revue.
#[tauri::command]
#[instrument(skip(vault))]
pub fn mcp_draft(
    vault: State<'_, VaultStateArc>,
    name: String,
    description: String,
    body: String,
) -> Result<String, String> {
    vault.mcp_draft(&name, &description, &body)
}

// ===== Commandes — Graphe du coffre =====

/// Graphe du coffre : notes, liens résolus, liens cassés et étiquettes.
///
/// Reconstruit depuis les fichiers Markdown. Obsidian n'expose pas d'API
/// externe, et un plugin communautaire exigerait qu'il tourne sans fournir
/// pour autant le graphe déjà dessiné.
#[tauri::command]
#[instrument(skip(vault))]
pub fn vault_graph(vault: State<'_, VaultStateArc>) -> Result<VaultGraph, String> {
    vault.graph()
}

/// Ouvre une note dans Obsidian.
///
/// Passe par `xdg-open`, natif Linux, plutôt que par un greffon supplémentaire
/// pour un seul bouton. L'argument est transmis directement au programme,
/// sans interprétation par un shell — et le chemin est d'abord validé par la
/// sandbox du coffre, donc il ne peut pas désigner un fichier extérieur.
#[tauri::command]
#[instrument(skip(vault))]
pub fn vault_open_external(
    vault: State<'_, VaultStateArc>,
    rel_path: String,
) -> Result<String, String> {
    let chemin = vault.safe_join(&rel_path, true)?;
    let uri = crate::graph::uri_obsidian(&chemin.to_string_lossy());
    std::process::Command::new("xdg-open")
        .arg(&uri)
        .spawn()
        .map_err(|e| format!("ouverture impossible ({e}) — Obsidian est-il installé ?"))?;
    info!(note = %rel_path, "note ouverte dans Obsidian");
    Ok(uri)
}

// ===== Commandes — Intendant =====

/// Résultat de l'application d'une action.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    pub description: String,
    pub ok: bool,
    pub error: Option<String>,
}

/// Prompt système de l'intendant, tel qu'il sera envoyé.
///
/// Exposé pour que l'utilisateur puisse le lire : un agent qui reconfigure
/// l'application ne doit pas être une boîte noire.
#[tauri::command]
#[instrument(skip(vault, registry))]
pub async fn intendant_prompt(
    vault: State<'_, VaultStateArc>,
    registry: State<'_, ProviderRegistryState>,
) -> Result<String, String> {
    Ok(intendant::prompt(
        &agents_connus(&vault),
        &modeles_connus(&registry).await,
        &mcp_connus(&vault),
    ))
}

fn agents_connus(vault: &VaultState) -> Vec<String> {
    vault
        .agents_list()
        .unwrap_or_default()
        .into_iter()
        .map(|a| a.name)
        .collect()
}

/// Outils MCP déjà déclarés : l'intendant doit savoir ce qui existe pour ne
/// pas proposer de redéclarer un outil que le coffre possède.
fn mcp_connus(vault: &VaultState) -> Vec<String> {
    vault
        .mcp_list()
        .unwrap_or_default()
        .into_iter()
        .map(|m| m.name)
        .collect()
}

async fn modeles_connus(registry: &ProviderRegistry) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    for p in registry.list_all().await {
        let modeles = p
            .list_models()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|m| m.id)
            .collect();
        out.push((p.id().to_string(), modeles));
    }
    out
}

/// Envoie un message à l'intendant et renvoie les actions qu'il propose.
///
/// Rien n'est appliqué ici : la proposition est décrite en clair et attend une
/// validation. Un modèle qui se trompe de thème est sans conséquence ; un
/// modèle qui supprime une planification par contresens, beaucoup moins.
#[tauri::command]
#[instrument(skip(bus, sessions, pool, vault, registry))]
pub async fn intendant_send(
    bus: State<'_, EventBusState>,
    sessions: State<'_, SessionManagerState>,
    pool: State<'_, ProviderPoolState>,
    vault: State<'_, VaultStateArc>,
    registry: State<'_, ProviderRegistryState>,
    session_id: String,
    content: String,
) -> Result<Option<intendant::Proposition>, String> {
    let prompt = intendant::prompt(
        &agents_connus(&vault),
        &modeles_connus(&registry).await,
        &mcp_connus(&vault),
    );
    let texte = session_send_impl(
        bus.inner(),
        sessions.inner(),
        pool.inner(),
        vault.inner(),
        session_id.clone(),
        content,
        Some(&prompt),
    )
    .await?;

    match intendant::extraire_actions(&texte) {
        Ok(proposition) => {
            if let Some(p) = &proposition {
                info!(actions = p.actions.len(), "l'intendant propose des actions");
            }
            Ok(proposition)
        }
        // Une proposition mal formée est signalée sans faire échouer le tour :
        // la réponse en langage naturel, elle, reste utile.
        Err(e) => {
            warn!(%e, "proposition de l'intendant écartée");
            bus.emit(
                "intendant:refus",
                serde_json::json!({ "sessionId": session_id, "raison": e }),
            );
            Ok(None)
        }
    }
}

/// Applique les actions validées par l'utilisateur.
///
/// Chaque action est indépendante : l'échec de l'une n'annule pas les autres,
/// et le résultat est rendu action par action pour que l'on sache exactement
/// ce qui a pris effet.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
#[instrument(skip(
    config,
    sessions,
    teams,
    plans,
    plugins,
    remote_state,
    bus,
    pool,
    registry,
    vault
))]
pub async fn intendant_apply(
    config: State<'_, ConfigStateArc>,
    sessions: State<'_, SessionManagerState>,
    teams: State<'_, TeamStoreState>,
    plans: State<'_, PlanManagerState>,
    plugins: State<'_, PluginStoreState>,
    remote_state: State<'_, RemoteStateArc>,
    bus: State<'_, EventBusState>,
    pool: State<'_, ProviderPoolState>,
    registry: State<'_, ProviderRegistryState>,
    vault: State<'_, VaultStateArc>,
    actions: Vec<intendant::Action>,
) -> Result<Vec<ActionResult>, String> {
    let mut resultats = Vec::new();
    for action in actions {
        let description = action.describe();
        let issue = appliquer(
            &action,
            config.inner(),
            sessions.inner(),
            teams.inner(),
            plans.inner(),
            plugins.inner(),
            remote_state.inner(),
            bus.inner(),
            pool.inner(),
            registry.inner(),
            vault.inner(),
        )
        .await;
        match issue {
            Ok(()) => {
                info!(%description, "action appliquée");
                resultats.push(ActionResult {
                    description,
                    ok: true,
                    error: None,
                });
            }
            Err(e) => {
                warn!(%description, %e, "action refusée");
                resultats.push(ActionResult {
                    description,
                    ok: false,
                    error: Some(e),
                });
            }
        }
    }
    Ok(resultats)
}

/// Applique une action. Les validations de fond restent dans les modules
/// concernés — équipe cohérente, plan sans cycle, valeur CSS sûre : les
/// refaire ici les ferait diverger de la référence.
#[allow(clippy::too_many_arguments)]
async fn appliquer(
    action: &intendant::Action,
    config: &ConfigStateArc,
    sessions: &SessionManagerState,
    teams: &TeamStoreState,
    plans: &PlanManagerState,
    plugins: &PluginStoreState,
    remote_state: &RemoteStateArc,
    bus: &EventBusState,
    pool: &ProviderPoolState,
    registry: &ProviderRegistryState,
    vault: &VaultStateArc,
) -> Result<(), String> {
    use intendant::Action;
    action.validate()?;
    match action {
        Action::Theme { theme } => {
            config.update(crate::config::ConfigPatch {
                theme: Some(theme.clone()),
                ..Default::default()
            })?;
            Ok(())
        }
        Action::FournisseurDefaut { provider_id } => {
            // Un fournisseur inexistant laisserait l'interface sans modèle
            // sélectionnable, sans dire pourquoi.
            if registry.get(provider_id).await.is_none() {
                return Err(format!("fournisseur inconnu : {provider_id}"));
            }
            config.update(crate::config::ConfigPatch {
                default_provider: Some(provider_id.clone()),
                ..Default::default()
            })?;
            Ok(())
        }
        Action::Session {
            agent,
            provider,
            model,
        } => {
            sessions
                .create(agent.clone(), None, provider.clone(), model.clone())
                .await?;
            Ok(())
        }
        Action::Equipe {
            name,
            description,
            members,
            strategy,
            max_turns,
        } => teams.save(&Team::new(
            name.clone(),
            description.clone(),
            members.clone(),
            *strategy,
            *max_turns,
        )),
        Action::Planification {
            title,
            objective,
            steps,
        } => {
            plans
                .save(&Plan::new(title.clone(), objective.clone(), steps.clone()))
                .await
        }
        Action::Patch {
            name,
            description,
            theme,
        } => {
            let mut patch = UiPatch::new(name.clone(), description.clone());
            patch.theme = theme.clone();
            plugins.save_patch(&patch)
        }
        Action::PatchActif { patch_id, enabled } => {
            let mut patch = plugins.load_patch(patch_id)?;
            patch.enabled = *enabled;
            plugins.save_patch(&patch)?;
            // La fenêtre applique le CSS cumulé sans avoir à redemander.
            bus.emit("patch:css", plugins.css_actif());
            Ok(())
        }
        Action::Mcp {
            name,
            description,
            body,
        } => {
            // `mcp_draft` vise `brouillon/IA/MCP/` et la sandbox du coffre
            // refuse tout le reste : l'intendant ne peut pas écrire ailleurs
            // même s'il le demandait.
            vault.mcp_draft(name, description, body).map(|_| ())
        }
        Action::Distant { enabled } => {
            if !*enabled {
                remote_state.arreter().await;
                config.update(crate::config::ConfigPatch {
                    remote_enabled: Some(false),
                    ..Default::default()
                })?;
                return Ok(());
            }
            let cfg = config.read();
            let jeton = match cfg.remote_token() {
                Some(j) => j,
                None => {
                    // Il n'existe pas de mode sans authentification, même
                    // demandé par le chat.
                    let j = remote::generer_jeton();
                    config.set_remote_token(j.clone())?;
                    j
                }
            };
            let h = Harness {
                bus: Arc::clone(bus),
                sessions: Arc::clone(sessions),
                teams: Arc::clone(teams),
                plans: Arc::clone(plans),
                pool: Arc::clone(pool),
                registry: Arc::clone(registry),
                vault: Arc::clone(vault),
            };
            remote_state
                .demarrer(h, &bind_configure(&cfg), &jeton)
                .await?;
            config.update(crate::config::ConfigPatch {
                remote_enabled: Some(true),
                ..Default::default()
            })?;
            Ok(())
        }
    }
}
