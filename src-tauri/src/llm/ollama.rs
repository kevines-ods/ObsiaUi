//! Provider Ollama.
//!
//! Implémenté directement sur l'API HTTP native (`/api/chat`, `/api/tags`,
//! `/api/version`) plutôt que sur un client tiers, pour deux raisons :
//!
//! - **L'adresse détectée doit être respectée.** La détection
//!   (`discovery.rs`) peut trouver un daemon sur `gpu.lan:11434` ; un client
//!   figé sur `127.0.0.1` enverrait le chat au mauvais hôte tout en
//!   rapportant une liste de modèles correcte — panne silencieuse.
//! - **Le streaming doit être réel.** `/api/chat` répond en NDJSON (un objet
//!   JSON par ligne) : les jetons remontent au fil de l'eau, au lieu d'un
//!   unique bloc à la fin de la génération.

use crate::llm::provider::{
    ChatChoice, ChatMessage, ChatRequest, ChatResponse, LlmError, LlmProvider, ModelCapability,
    ModelInfo, TokenEvent, TokenStream, Usage,
};
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::json;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{info, instrument, warn};

/// Contexte supposé quand le daemon ne le rapporte pas.
const FALLBACK_CONTEXT: u32 = 4096;

pub struct OllamaProvider {
    base_url: String,
    client: reqwest::Client,
}

impl OllamaProvider {
    /// Construit le provider sur une URL de base normalisée.
    pub fn new(base_url: impl Into<String>) -> Self {
        let raw = base_url.into();
        // Tolère `127.0.0.1:11434` sans schéma, forme courante d'`OLLAMA_HOST`.
        let base_url =
            crate::discovery::normalize_base_url(&raw, crate::discovery::OLLAMA_DEFAULT_PORT)
                .unwrap_or_else(|| "http://127.0.0.1:11434".to_string());

        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            // Un daemon local ne passe pas par le proxy du poste.
            .no_proxy()
            .build()
            .unwrap_or_default();
        Self { base_url, client }
    }

    /// Adresse issue de l'environnement, avec repli sur le port conventionnel.
    pub fn from_env() -> Self {
        Self::new(std::env::var("OLLAMA_HOST").unwrap_or_default())
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Corps `/api/chat`. Ollama place les réglages sous `options`, pas à la
    /// racine comme OpenAI.
    fn body(req: &ChatRequest, stream: bool) -> serde_json::Value {
        let messages: Vec<serde_json::Value> = req
            .messages
            .iter()
            .map(|m| json!({ "role": normalize_role(&m.role), "content": m.content }))
            .collect();
        let mut options = serde_json::Map::new();
        if let Some(t) = req.temperature {
            options.insert("temperature".into(), json!(t));
        }
        if let Some(m) = req.max_tokens {
            options.insert("num_predict".into(), json!(m));
        }
        let mut body = json!({
            "model": req.model,
            "messages": messages,
            "stream": stream,
        });
        if !options.is_empty() {
            body["options"] = serde_json::Value::Object(options);
        }
        body
    }
}

/// Ollama n'accepte que `system`, `user`, `assistant` et `tool`.
fn normalize_role(role: &str) -> &str {
    match role {
        "system" | "assistant" | "user" | "tool" => role,
        _ => "user",
    }
}

/// Assembleur de flux NDJSON : découpe sur les sauts de ligne en tamponnant
/// les lignes incomplètes (un objet JSON peut être coupé entre deux chunks).
#[derive(Default)]
pub struct NdjsonDecoder {
    buffer: String,
}

impl NdjsonDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ajoute un fragment et renvoie les lignes complètes, vides exclues.
    pub fn push(&mut self, chunk: &str) -> Vec<String> {
        self.buffer.push_str(chunk);
        let mut out = Vec::new();
        while let Some(pos) = self.buffer.find('\n') {
            let line: String = self.buffer.drain(..pos + 1).collect();
            let line = line.trim();
            if !line.is_empty() {
                out.push(line.to_string());
            }
        }
        out
    }
}

/// Fragment de texte d'un objet `/api/chat` en flux.
pub fn message_content(chunk: &serde_json::Value) -> Option<String> {
    chunk["message"]["content"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Compteurs de jetons rapportés par Ollama en fin de génération.
pub fn usage_from(chunk: &serde_json::Value) -> Option<Usage> {
    let prompt = chunk["prompt_eval_count"].as_u64();
    let completion = chunk["eval_count"].as_u64();
    if prompt.is_none() && completion.is_none() {
        return None;
    }
    let prompt = prompt.unwrap_or(0) as u32;
    let completion = completion.unwrap_or(0) as u32;
    Some(Usage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: prompt + completion,
    })
}

/// Convertit une entrée `/api/tags` en modèle du registry.
pub fn model_from_tag(tag: &serde_json::Value) -> Option<ModelInfo> {
    let id = tag["name"].as_str()?;
    let context_window = tag["details"]["context_length"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(FALLBACK_CONTEXT);
    // La famille sert à deviner les capacités : les modèles de vision
    // d'Ollama portent `clip`/`vision` dans leurs familles déclarées.
    let families = tag["details"]["families"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|f| f.as_str())
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut capabilities = vec![ModelCapability::Chat];
    if families.contains("clip") || families.contains("vision") {
        capabilities.push(ModelCapability::Vision);
    }
    if id.contains("embed") {
        capabilities.push(ModelCapability::Embedding);
    }
    Some(ModelInfo {
        id: id.to_string(),
        name: id.to_string(),
        provider: "ollama".to_string(),
        context_window,
        capabilities,
        pricing: None,
        local_path: None,
    })
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn id(&self) -> &str {
        "ollama"
    }

    fn name(&self) -> &str {
        "Ollama (local)"
    }

    #[instrument(skip(self))]
    async fn health_check(&self) -> Result<(), LlmError> {
        let resp = self
            .client
            .get(format!("{}/api/version", self.base_url))
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| LlmError::ProviderUnavailable(e.to_string()))?;
        if resp.status().is_success() {
            info!(base_url = %self.base_url, "Ollama joignable");
            Ok(())
        } else {
            Err(LlmError::ProviderUnavailable(format!(
                "Ollama a répondu HTTP {}",
                resp.status().as_u16()
            )))
        }
    }

    #[instrument(skip(self))]
    async fn list_models(&self) -> Result<Vec<ModelInfo>, LlmError> {
        let resp = self
            .client
            .get(format!("{}/api/tags", self.base_url))
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| LlmError::ProviderUnavailable(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(LlmError::ApiError(format!(
                "/api/tags a répondu HTTP {}",
                resp.status().as_u16()
            )));
        }
        let payload: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| LlmError::ApiError(e.to_string()))?;
        let models: Vec<ModelInfo> = payload["models"]
            .as_array()
            .map(|arr| arr.iter().filter_map(model_from_tag).collect())
            .unwrap_or_default();
        if models.is_empty() {
            warn!(base_url = %self.base_url, "Ollama ne déclare aucun modèle (ollama pull ?)");
        }
        Ok(models)
    }

    #[instrument(skip(self, req))]
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, LlmError> {
        let resp = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&Self::body(&req, false))
            .send()
            .await
            .map_err(|e| LlmError::ProviderUnavailable(e.to_string()))?;
        let status = resp.status();
        let payload: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| LlmError::ApiError(e.to_string()))?;
        if !status.is_success() {
            let detail = payload["error"].as_str().unwrap_or("erreur inconnue");
            return Err(LlmError::ApiError(format!("HTTP {status} — {detail}")));
        }
        Ok(ChatResponse {
            id: uuid::Uuid::new_v4().to_string(),
            model: payload["model"].as_str().unwrap_or(&req.model).to_string(),
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: message_content(&payload).unwrap_or_default(),
                    name: None,
                },
                finish_reason: payload["done_reason"]
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| Some("stop".to_string())),
            }],
            usage: usage_from(&payload),
        })
    }

    #[instrument(skip(self, req))]
    async fn chat_stream(&self, req: ChatRequest) -> Result<TokenStream, LlmError> {
        let (tx, rx) = mpsc::channel(100);
        let model = req.model.clone();
        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&Self::body(&req, true))
            .send()
            .await
            .map_err(|e| LlmError::ProviderUnavailable(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let detail = response.text().await.unwrap_or_default();
            return Err(LlmError::ApiError(format!(
                "HTTP {status} — {}",
                detail.trim()
            )));
        }

        tokio::spawn(async move {
            let mut decoder = NdjsonDecoder::new();
            let mut full = String::new();
            let mut stream = response.bytes_stream();

            while let Some(chunk) = stream.next().await {
                let bytes = match chunk {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = tx.send(TokenEvent::Error(e.to_string())).await;
                        return;
                    }
                };
                for line in decoder.push(&String::from_utf8_lossy(&bytes)) {
                    let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                        warn!(%line, "ligne NDJSON Ollama illisible, ignorée");
                        continue;
                    };
                    if let Some(err) = value["error"].as_str() {
                        let _ = tx.send(TokenEvent::Error(err.to_string())).await;
                        return;
                    }
                    if let Some(token) = message_content(&value) {
                        full.push_str(&token);
                        if tx.send(TokenEvent::Token(token)).await.is_err() {
                            // Récepteur fermé : session annulée côté appelant.
                            return;
                        }
                    }
                    if value["done"].as_bool() == Some(true) {
                        let _ = tx
                            .send(TokenEvent::Done(ChatResponse {
                                id: uuid::Uuid::new_v4().to_string(),
                                model: value["model"].as_str().unwrap_or(&model).to_string(),
                                choices: vec![ChatChoice {
                                    index: 0,
                                    message: ChatMessage {
                                        role: "assistant".to_string(),
                                        content: std::mem::take(&mut full),
                                        name: None,
                                    },
                                    finish_reason: value["done_reason"]
                                        .as_str()
                                        .map(str::to_string)
                                        .or_else(|| Some("stop".to_string())),
                                }],
                                usage: usage_from(&value),
                            }))
                            .await;
                        return;
                    }
                }
            }
            // Flux interrompu sans `done` : on livre ce qui a été reçu.
            let _ = tx
                .send(TokenEvent::Done(ChatResponse {
                    id: uuid::Uuid::new_v4().to_string(),
                    model,
                    choices: vec![ChatChoice {
                        index: 0,
                        message: ChatMessage {
                            role: "assistant".to_string(),
                            content: full,
                            name: None,
                        },
                        finish_reason: Some("incomplete".to_string()),
                    }],
                    usage: None,
                }))
                .await;
        });

        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn request() -> ChatRequest {
        ChatRequest {
            model: "qwen3:8b".into(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "salut".into(),
                name: None,
            }],
            temperature: Some(0.3),
            max_tokens: Some(128),
            stream: false,
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn honore_l_adresse_fournie() {
        // Régression : le client ignorait l'URL et parlait toujours au
        // bouclage, ce qui envoyait le chat au mauvais hôte.
        let p = OllamaProvider::new("http://gpu.lan:11434");
        assert_eq!(p.base_url(), "http://gpu.lan:11434");
    }

    #[test]
    fn accepte_une_adresse_sans_schema() {
        let p = OllamaProvider::new("gpu.lan:11434");
        assert_eq!(p.base_url(), "http://gpu.lan:11434");
    }

    #[test]
    fn retombe_sur_le_bouclage_si_l_adresse_est_vide() {
        assert_eq!(OllamaProvider::new("").base_url(), "http://127.0.0.1:11434");
    }

    #[test]
    fn les_reglages_vont_dans_options() {
        // Placés à la racine, Ollama les ignore silencieusement.
        let body = OllamaProvider::body(&request(), true);
        assert_eq!(body["stream"], true);
        // f32 élargi en f64 : comparer à epsilon, pas à l'identique.
        assert!((body["options"]["temperature"].as_f64().unwrap() - 0.3).abs() < 1e-6);
        assert_eq!(body["options"]["num_predict"], 128);
    }

    #[test]
    fn pas_d_options_quand_aucun_reglage() {
        let mut req = request();
        req.temperature = None;
        req.max_tokens = None;
        assert!(OllamaProvider::body(&req, false).get("options").is_none());
    }

    #[test]
    fn decoupe_le_ndjson_ligne_a_ligne() {
        let mut d = NdjsonDecoder::new();
        assert_eq!(
            d.push("{\"a\":1}\n{\"b\":2}\n"),
            vec!["{\"a\":1}", "{\"b\":2}"]
        );
    }

    #[test]
    fn tamponne_une_ligne_coupee() {
        let mut d = NdjsonDecoder::new();
        assert!(d.push("{\"cont").is_empty());
        assert_eq!(d.push("enu\":1}\n"), vec!["{\"contenu\":1}"]);
    }

    #[test]
    fn extrait_le_fragment_de_message() {
        let chunk = serde_json::json!({"message": {"content": "bon"}});
        assert_eq!(message_content(&chunk).as_deref(), Some("bon"));
        let vide = serde_json::json!({"message": {"content": ""}});
        assert_eq!(message_content(&vide), None);
    }

    #[test]
    fn lit_les_compteurs_de_jetons() {
        let done = serde_json::json!({"done": true, "prompt_eval_count": 10, "eval_count": 5});
        let usage = usage_from(&done).unwrap();
        assert_eq!(usage.total_tokens, 15);
        assert!(usage_from(&serde_json::json!({"done": true})).is_none());
    }

    #[test]
    fn convertit_une_entree_de_tags() {
        let tag = serde_json::json!({
            "name": "qwen3:8b",
            "details": {"context_length": 32768, "families": ["qwen3"]}
        });
        let model = model_from_tag(&tag).unwrap();
        assert_eq!(model.id, "qwen3:8b");
        assert_eq!(model.context_window, 32768);
        assert_eq!(model.capabilities, vec![ModelCapability::Chat]);
    }

    #[test]
    fn deduit_la_capacite_vision_de_la_famille() {
        let tag = serde_json::json!({
            "name": "llava:7b",
            "details": {"families": ["llama", "clip"]}
        });
        let model = model_from_tag(&tag).unwrap();
        assert!(model.capabilities.contains(&ModelCapability::Vision));
    }

    #[test]
    fn ignore_une_entree_sans_nom() {
        assert!(model_from_tag(&serde_json::json!({"size": 42})).is_none());
    }
}
