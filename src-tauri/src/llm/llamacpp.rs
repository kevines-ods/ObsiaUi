//! Provider llama.cpp (`llama-server`).
//!
//! `llama-server` expose deux surfaces : une API native (`/health`, `/props`,
//! `/completion`) et une API **compatible OpenAI** (`/v1/chat/completions`).
//! On s'appuie sur la seconde pour le chat — c'est la plus stable d'une
//! version à l'autre — et sur la première pour la santé et les métadonnées
//! (taille de contexte, nom du modèle chargé), qu'OpenAI ne couvre pas.
//!
//! On n'utilise pas `async-openai` ici : ce client impose une clé d'API et
//! une forme d'erreur pensées pour api.openai.com, alors qu'un serveur local
//! tourne le plus souvent **sans authentification**.

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

/// Contexte supposé quand `/props` ne le rapporte pas (défaut amont de
/// `llama-server`).
const FALLBACK_CONTEXT: u32 = 4096;

pub struct LlamaCppProvider {
    base_url: String,
    client: reqwest::Client,
    /// Jeton `--api-key` du serveur, quand il en exige un.
    api_key: Option<String>,
}

impl LlamaCppProvider {
    /// Construit un provider sur une URL de base déjà normalisée
    /// (cf. `discovery::normalize_base_url`), sans chemin final.
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            // Une génération longue peut dépasser la minute sur CPU : seul le
            // délai de connexion est borné court, pas la lecture.
            .connect_timeout(Duration::from_secs(5))
            .no_proxy()
            .build()
            .unwrap_or_default();
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            client,
            api_key: api_key.filter(|k| !k.trim().is_empty()),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        let req = self.client.post(format!("{}{path}", self.base_url));
        match &self.api_key {
            Some(k) => req.bearer_auth(k),
            None => req,
        }
    }

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        let req = self.client.get(format!("{}{path}", self.base_url));
        match &self.api_key {
            Some(k) => req.bearer_auth(k),
            None => req,
        }
    }

    /// Corps `/v1/chat/completions`. `stream` bascule la forme de la réponse.
    fn body(req: &ChatRequest, stream: bool) -> serde_json::Value {
        let messages: Vec<serde_json::Value> = req
            .messages
            .iter()
            .map(|m| json!({ "role": normalize_role(&m.role), "content": m.content }))
            .collect();
        let mut body = json!({
            "model": req.model,
            "messages": messages,
            "stream": stream,
        });
        if let Some(t) = req.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(m) = req.max_tokens {
            body["max_tokens"] = json!(m);
        }
        body
    }

    /// Taille de contexte réellement chargée, lue sur `/props`.
    async fn context_window(&self) -> u32 {
        let Ok(resp) = self.get("/props").send().await else {
            return FALLBACK_CONTEXT;
        };
        if !resp.status().is_success() {
            return FALLBACK_CONTEXT;
        }
        let Ok(v) = resp.json::<serde_json::Value>().await else {
            return FALLBACK_CONTEXT;
        };
        v["default_generation_settings"]["n_ctx"]
            .as_u64()
            .or_else(|| v["n_ctx"].as_u64())
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(FALLBACK_CONTEXT)
    }
}

/// llama.cpp n'accepte que les rôles OpenAI : tout rôle inconnu devient
/// `user` plutôt que de faire échouer la requête entière.
fn normalize_role(role: &str) -> &str {
    match role {
        "system" | "assistant" | "user" | "tool" => role,
        _ => "user",
    }
}

// ===== Décodage SSE =====

/// Assembleur d'événements `text/event-stream`.
///
/// Les chunks HTTP ne s'alignent pas sur les frontières d'événements : un
/// `data:` peut arriver coupé en deux. Ce décodeur tamponne jusqu'à obtenir
/// une trame complète (séparateur ligne vide) et renvoie les charges utiles
/// des champs `data:`.
#[derive(Default)]
pub struct SseDecoder {
    buffer: String,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ajoute un fragment et renvoie les charges `data:` désormais complètes.
    pub fn push(&mut self, chunk: &str) -> Vec<String> {
        // Normalisation CRLF : les serveurs SSE mélangent les deux formes.
        self.buffer.push_str(&chunk.replace("\r\n", "\n"));
        let mut out = Vec::new();
        while let Some(pos) = self.buffer.find("\n\n") {
            let frame: String = self.buffer.drain(..pos + 2).collect();
            if let Some(data) = Self::frame_data(&frame) {
                out.push(data);
            }
        }
        out
    }

    /// Concatène les champs `data:` d'une trame (une trame peut en porter
    /// plusieurs, à recoller par des sauts de ligne selon la spécification).
    fn frame_data(frame: &str) -> Option<String> {
        let mut parts = Vec::new();
        for line in frame.lines() {
            if let Some(rest) = line.strip_prefix("data:") {
                parts.push(rest.strip_prefix(' ').unwrap_or(rest));
            }
        }
        if parts.is_empty() {
            return None;
        }
        Some(parts.join("\n"))
    }
}

/// Extrait le fragment de texte d'un chunk OpenAI `chat.completion.chunk`.
pub fn delta_content(chunk: &serde_json::Value) -> Option<String> {
    chunk["choices"][0]["delta"]["content"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Extrait la raison d'arrêt d'un chunk, quand elle est présente.
pub fn finish_reason(chunk: &serde_json::Value) -> Option<String> {
    chunk["choices"][0]["finish_reason"]
        .as_str()
        .map(str::to_string)
}

// ===== Implémentation du trait =====

#[async_trait]
impl LlmProvider for LlamaCppProvider {
    fn id(&self) -> &str {
        "llamacpp"
    }

    fn name(&self) -> &str {
        "llama.cpp (local)"
    }

    fn supported_capabilities(&self) -> Vec<ModelCapability> {
        vec![ModelCapability::Chat, ModelCapability::Completion]
    }

    #[instrument(skip(self))]
    async fn health_check(&self) -> Result<(), LlmError> {
        let resp = self
            .get("/health")
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| LlmError::ProviderUnavailable(e.to_string()))?;
        match resp.status().as_u16() {
            200 => {
                info!(base_url = %self.base_url, "llama.cpp joignable");
                Ok(())
            }
            // 503 = le serveur est bien là, le modèle finit de charger.
            503 => Err(LlmError::ProviderUnavailable(
                "llama.cpp charge encore le modèle".into(),
            )),
            401 | 403 => Err(LlmError::AuthFailed(
                "llama.cpp exige une clé (--api-key)".into(),
            )),
            code => Err(LlmError::ProviderUnavailable(format!(
                "llama.cpp a répondu HTTP {code}"
            ))),
        }
    }

    #[instrument(skip(self))]
    async fn list_models(&self) -> Result<Vec<ModelInfo>, LlmError> {
        let resp = self
            .get("/v1/models")
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| LlmError::ProviderUnavailable(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(LlmError::ApiError(format!(
                "/v1/models a répondu HTTP {}",
                resp.status().as_u16()
            )));
        }
        let payload: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| LlmError::ApiError(e.to_string()))?;
        let context_window = self.context_window().await;

        let models: Vec<ModelInfo> = payload["data"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m["id"].as_str())
                    .map(|id| ModelInfo {
                        id: id.to_string(),
                        // Un `id` de llama.cpp est un chemin de fichier :
                        // on affiche le nom du modèle, pas le chemin complet.
                        name: crate::discovery::model_name_from_path(id),
                        provider: "llamacpp".to_string(),
                        context_window,
                        capabilities: vec![ModelCapability::Chat],
                        pricing: None,
                        local_path: Some(id.to_string()),
                    })
                    .collect()
            })
            .unwrap_or_default();

        if models.is_empty() {
            warn!(base_url = %self.base_url, "llama.cpp ne déclare aucun modèle");
        }
        Ok(models)
    }

    #[instrument(skip(self, req))]
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, LlmError> {
        let resp = self
            .post("/v1/chat/completions")
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
            let detail = payload["error"]["message"]
                .as_str()
                .unwrap_or("erreur inconnue");
            return Err(LlmError::ApiError(format!("HTTP {status} — {detail}")));
        }
        Ok(parse_completion(&payload, &req.model))
    }

    #[instrument(skip(self, req))]
    async fn chat_stream(&self, req: ChatRequest) -> Result<TokenStream, LlmError> {
        let (tx, rx) = mpsc::channel(100);
        let model = req.model.clone();
        let response = self
            .post("/v1/chat/completions")
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
            let mut decoder = SseDecoder::new();
            let mut full = String::new();
            let mut reason: Option<String> = None;
            let mut stream = response.bytes_stream();

            while let Some(chunk) = stream.next().await {
                let bytes = match chunk {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = tx.send(TokenEvent::Error(e.to_string())).await;
                        return;
                    }
                };
                for data in decoder.push(&String::from_utf8_lossy(&bytes)) {
                    if data.trim() == "[DONE]" {
                        let _ = tx
                            .send(TokenEvent::Done(build_response(&model, full, reason)))
                            .await;
                        return;
                    }
                    let Ok(value) = serde_json::from_str::<serde_json::Value>(&data) else {
                        // Une trame illisible ne doit pas tuer le flux entier.
                        warn!(%data, "chunk SSE llama.cpp illisible, ignoré");
                        continue;
                    };
                    if let Some(err) = value["error"]["message"].as_str() {
                        let _ = tx.send(TokenEvent::Error(err.to_string())).await;
                        return;
                    }
                    if let Some(token) = delta_content(&value) {
                        full.push_str(&token);
                        if tx.send(TokenEvent::Token(token)).await.is_err() {
                            // Récepteur fermé : la session a été annulée.
                            return;
                        }
                    }
                    if let Some(r) = finish_reason(&value) {
                        reason = Some(r);
                    }
                }
            }
            // Flux clos sans `[DONE]` : on livre quand même ce qui a été reçu.
            let _ = tx
                .send(TokenEvent::Done(build_response(&model, full, reason)))
                .await;
        });

        Ok(rx)
    }
}

/// Construit la réponse finale d'un flux à partir du texte accumulé.
fn build_response(model: &str, content: String, finish_reason: Option<String>) -> ChatResponse {
    ChatResponse {
        id: uuid::Uuid::new_v4().to_string(),
        model: model.to_string(),
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content,
                name: None,
            },
            finish_reason: finish_reason.or_else(|| Some("stop".to_string())),
        }],
        usage: None,
    }
}

/// Convertit une réponse `/v1/chat/completions` non streamée.
fn parse_completion(payload: &serde_json::Value, fallback_model: &str) -> ChatResponse {
    let content = payload["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    ChatResponse {
        id: payload["id"].as_str().unwrap_or("llamacpp").to_string(),
        model: payload["model"]
            .as_str()
            .unwrap_or(fallback_model)
            .to_string(),
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content,
                name: None,
            },
            finish_reason: payload["choices"][0]["finish_reason"]
                .as_str()
                .map(str::to_string),
        }],
        usage: payload["usage"].as_object().map(|u| Usage {
            prompt_tokens: u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            completion_tokens: u
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            total_tokens: u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn request(model: &str) -> ChatRequest {
        ChatRequest {
            model: model.into(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "bonjour".into(),
                name: None,
            }],
            temperature: Some(0.2),
            max_tokens: Some(64),
            stream: false,
            metadata: HashMap::new(),
        }
    }

    // ===== SseDecoder =====

    #[test]
    fn assemble_une_trame_complete() {
        let mut d = SseDecoder::new();
        assert_eq!(d.push("data: {\"a\":1}\n\n"), vec!["{\"a\":1}"]);
    }

    #[test]
    fn tamponne_une_trame_coupee_entre_deux_chunks() {
        // Cas réel : le découpage HTTP tombe au milieu du JSON.
        let mut d = SseDecoder::new();
        assert!(d.push("data: {\"cont").is_empty());
        assert!(d.push("enu\":\"ok\"}").is_empty());
        assert_eq!(d.push("\n\n"), vec!["{\"contenu\":\"ok\"}"]);
    }

    #[test]
    fn rend_plusieurs_trames_d_un_seul_chunk() {
        let mut d = SseDecoder::new();
        assert_eq!(d.push("data: 1\n\ndata: 2\n\n"), vec!["1", "2"]);
    }

    #[test]
    fn accepte_les_fins_de_ligne_crlf() {
        let mut d = SseDecoder::new();
        assert_eq!(d.push("data: x\r\n\r\n"), vec!["x"]);
    }

    #[test]
    fn ignore_les_commentaires_de_maintien_de_connexion() {
        // Les serveurs SSE envoient `: ping` pour garder la connexion ouverte.
        let mut d = SseDecoder::new();
        assert!(d.push(": ping\n\n").is_empty());
    }

    #[test]
    fn recolle_les_data_multiples_d_une_meme_trame() {
        let mut d = SseDecoder::new();
        assert_eq!(d.push("data: a\ndata: b\n\n"), vec!["a\nb"]);
    }

    // ===== Extraction des chunks =====

    #[test]
    fn extrait_le_fragment_de_texte() {
        let chunk = serde_json::json!({"choices":[{"delta":{"content":"salut"}}]});
        assert_eq!(delta_content(&chunk).as_deref(), Some("salut"));
    }

    #[test]
    fn ignore_un_delta_vide() {
        // Le premier chunk ne porte que le rôle : il ne doit rien émettre.
        let chunk = serde_json::json!({"choices":[{"delta":{"role":"assistant"}}]});
        assert_eq!(delta_content(&chunk), None);
        let vide = serde_json::json!({"choices":[{"delta":{"content":""}}]});
        assert_eq!(delta_content(&vide), None);
    }

    #[test]
    fn lit_la_raison_d_arret() {
        let chunk = serde_json::json!({"choices":[{"delta":{},"finish_reason":"stop"}]});
        assert_eq!(finish_reason(&chunk).as_deref(), Some("stop"));
        let sans = serde_json::json!({"choices":[{"delta":{}}]});
        assert_eq!(finish_reason(&sans), None);
    }

    // ===== Corps de requête =====

    #[test]
    fn le_corps_porte_les_parametres_de_generation() {
        let body = LlamaCppProvider::body(&request("qwen"), true);
        assert_eq!(body["model"], "qwen");
        assert_eq!(body["stream"], true);
        // `temperature` est un f32 élargi en f64 : la comparaison exacte
        // échouerait sur la représentation (0.2f32 -> 0.20000000298...).
        assert!((body["temperature"].as_f64().unwrap() - 0.2).abs() < 1e-6);
        assert_eq!(body["max_tokens"], 64);
        assert_eq!(body["messages"][0]["role"], "user");
    }

    #[test]
    fn les_parametres_absents_ne_sont_pas_envoyes() {
        // Envoyer `null` ferait rejeter la requête par llama-server.
        let mut req = request("qwen");
        req.temperature = None;
        req.max_tokens = None;
        let body = LlamaCppProvider::body(&req, false);
        assert!(body.get("temperature").is_none());
        assert!(body.get("max_tokens").is_none());
    }

    #[test]
    fn un_role_inconnu_devient_user() {
        assert_eq!(normalize_role("system"), "system");
        assert_eq!(normalize_role("superviseur"), "user");
    }

    // ===== Parsing des réponses =====

    #[test]
    fn convertit_une_reponse_non_streamee() {
        let payload = serde_json::json!({
            "id": "chatcmpl-1",
            "model": "qwen",
            "choices": [{"message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 3, "completion_tokens": 1, "total_tokens": 4}
        });
        let resp = parse_completion(&payload, "défaut");
        assert_eq!(resp.id, "chatcmpl-1");
        assert_eq!(resp.model, "qwen");
        assert_eq!(resp.choices[0].message.content, "ok");
        assert_eq!(resp.usage.unwrap().total_tokens, 4);
    }

    #[test]
    fn retombe_sur_le_modele_demande_si_la_reponse_l_omet() {
        let payload = serde_json::json!({"choices": [{"message": {"content": "ok"}}]});
        assert_eq!(parse_completion(&payload, "défaut").model, "défaut");
    }

    #[test]
    fn la_reponse_de_flux_porte_le_texte_accumule() {
        let resp = build_response("qwen", "abc".into(), None);
        assert_eq!(resp.choices[0].message.content, "abc");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    // ===== Construction =====

    #[test]
    fn l_url_de_base_est_nettoyee_de_son_slash_final() {
        let p = LlamaCppProvider::new("http://127.0.0.1:8080/", None);
        assert_eq!(p.base_url(), "http://127.0.0.1:8080");
    }

    #[test]
    fn une_cle_vide_equivaut_a_pas_de_cle() {
        let p = LlamaCppProvider::new("http://x:8080", Some("   ".into()));
        assert!(p.api_key.is_none());
    }
}
