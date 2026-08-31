//! Sessions à distance : piloter cette instance depuis un autre poste.
//!
//! Le coffre et les modèles vivent souvent sur une machine (station avec GPU,
//! serveur de la maison) alors qu'on veut travailler depuis une autre. Plutôt
//! que d'exposer chaque moteur séparément, ObsiaUi expose **son propre
//! harness** : un client distant ouvre des sessions, lance des équipes et
//! exécute des plans sur l'hôte, et reçoit le même flux d'événements que la
//! fenêtre locale.
//!
//! Le serveur appelle les mêmes fonctions que les commandes Tauri
//! (`*_impl` dans `commands.rs`) : il n'y a pas de second chemin d'exécution à
//! maintenir, donc pas de dérive possible entre local et distant.
//!
//! # Position de sécurité
//!
//! - **Un jeton est toujours exigé**, y compris en écoute sur la boucle
//!   locale : sur une machine partagée, tout processus local peut atteindre
//!   `127.0.0.1`.
//! - **Ouvrir au réseau est explicite.** L'écoute par défaut est
//!   `127.0.0.1` ; toute autre adresse doit être saisie sciemment.
//! - **La comparaison du jeton est à temps constant** : un `==` sur des
//!   chaînes s'arrête au premier octet différent et laisse fuir sa longueur
//!   commune, ce qui se mesure sur un réseau local.
//! - **Le daemon ne se reconfigure pas à distance.** Ni `config_set`, ni les
//!   commandes `remote_*` ne sont exposées : détenir le jeton donne la main
//!   sur le travail, pas le droit de rebinder l'écoute, de lire les clés
//!   d'API ni de changer le jeton.

use crate::commands::{
    plan_run_impl, session_send_impl, team_run_impl, ProviderInfo, ProviderPoolState,
};
use crate::event::EventBus;
use crate::llm::registry::ProviderRegistry;
use crate::plan::PlanManager;
use crate::session::SessionManager;
use crate::team::TeamStore;
use crate::vault::VaultState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State as AxumState};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};
use tracing::{info, warn};

/// Port d'écoute par défaut.
pub const PORT_DEFAUT: u16 = 7420;

/// Longueur minimale d'un jeton, en caractères. 32 hexadécimaux valent
/// 128 bits : hors de portée d'une recherche exhaustive en ligne.
pub const JETON_MIN: usize = 32;

// ===== Harness partagé =====

/// Tout ce dont le serveur a besoin : les mêmes instances que la fenêtre.
#[derive(Clone)]
pub struct Harness {
    pub bus: Arc<EventBus>,
    pub sessions: Arc<SessionManager>,
    pub teams: Arc<TeamStore>,
    pub plans: Arc<PlanManager>,
    pub pool: ProviderPoolState,
    pub registry: Arc<ProviderRegistry>,
    pub vault: Arc<VaultState>,
}

// ===== Jetons =====

/// Engendre un jeton de 128 bits, en hexadécimal.
///
/// Construit depuis deux UUID v4, dont l'aléa vient du système d'exploitation.
pub fn generer_jeton() -> String {
    let a = uuid::Uuid::new_v4();
    let b = uuid::Uuid::new_v4();
    // 64 bits de chaque moitié : les champs de version et de variante d'un
    // UUID sont fixes et n'apportent pas d'entropie.
    let mut octets = [0u8; 16];
    octets[..8].copy_from_slice(&a.as_bytes()[8..]);
    octets[8..].copy_from_slice(&b.as_bytes()[8..]);
    octets.iter().map(|o| format!("{o:02x}")).collect()
}

/// Compare deux jetons en temps constant.
///
/// Un `==` sur des `str` s'arrête au premier octet différent : le temps de
/// réponse révèle alors la longueur du préfixe correct, ce qui suffit à
/// reconstruire un jeton octet par octet sur un réseau local.
pub fn jetons_egaux(attendu: &str, fourni: &str) -> bool {
    let a = attendu.as_bytes();
    let b = fourni.as_bytes();
    // La différence de longueur est elle-même accumulée plutôt que court-
    // circuitée, et l'on parcourt toujours le jeton attendu en entier.
    let mut diff = (a.len() ^ b.len()) as u32;
    for (i, octet) in a.iter().enumerate() {
        let autre = b.get(i).copied().unwrap_or(0);
        diff |= (octet ^ autre) as u32;
    }
    diff == 0 && !a.is_empty()
}

/// Valide l'adresse d'écoute et le jeton avant de démarrer.
///
/// Renvoie l'adresse résolue, ou la raison du refus.
pub fn valider_ecoute(bind: &str, jeton: &str) -> Result<SocketAddr, String> {
    let bind = bind.trim();
    let bind = if bind.is_empty() {
        format!("127.0.0.1:{PORT_DEFAUT}")
    } else if bind.contains(':') {
        bind.to_string()
    } else {
        // Adresse seule : on complète avec le port conventionnel.
        format!("{bind}:{PORT_DEFAUT}")
    };
    let addr: SocketAddr = bind
        .parse()
        .map_err(|_| format!("adresse d'écoute invalide : {bind}"))?;

    if jeton.trim().len() < JETON_MIN {
        return Err(format!(
            "un jeton d'au moins {JETON_MIN} caractères est exigé, y compris \
             en écoute locale : sur une machine partagée, tout processus local \
             peut atteindre 127.0.0.1"
        ));
    }
    Ok(addr)
}

/// Vrai si l'adresse n'écoute que la boucle locale.
pub fn est_boucle(addr: &SocketAddr) -> bool {
    addr.ip().is_loopback()
}

// ===== Protocole =====

#[derive(Debug, Clone, Deserialize)]
pub struct RpcRequest {
    pub command: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RpcResponse {
    fn succes(v: Value) -> Self {
        Self {
            ok: true,
            result: Some(v),
            error: None,
        }
    }
    fn echec(e: String) -> Self {
        Self {
            ok: false,
            result: None,
            error: Some(e),
        }
    }
}

/// Commandes qu'un client distant peut invoquer.
///
/// Liste blanche explicite : une commande absente d'ici est refusée. En
/// particulier `config_set` et les commandes `remote_*` n'y figurent pas —
/// détenir le jeton donne la main sur le travail, pas sur la configuration de
/// l'hôte ni sur ses secrets.
pub const COMMANDES_AUTORISEES: &[&str] = &[
    "providers_list",
    "provider_test",
    "models_list",
    "llm_health_check",
    "runtimes_detect",
    "agents_list",
    "agent_read",
    "vault_list",
    "vault_read",
    "vault_write",
    "vault_path",
    "sessions_list",
    "session_create",
    "session_get",
    "session_rename",
    "session_delete",
    "session_send",
    "session_cancel",
    "session_export",
    "teams_list",
    "team_save",
    "team_delete",
    "team_run",
    "plans_list",
    "plan_save",
    "plan_delete",
    "plan_run",
    "plan_cancel",
];

pub fn commande_autorisee(nom: &str) -> bool {
    COMMANDES_AUTORISEES.contains(&nom)
}

fn params<T: DeserializeOwned>(v: Value) -> Result<T, String> {
    serde_json::from_value(v).map_err(|e| format!("paramètres invalides : {e}"))
}

fn valeur<T: Serialize>(v: T) -> Result<Value, String> {
    serde_json::to_value(v).map_err(|e| format!("réponse non sérialisable : {e}"))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdSession {
    session_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvoiSession {
    session_id: String,
    content: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenommageSession {
    session_id: String,
    title: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportSession {
    session_id: String,
    project: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecutionEquipe {
    session_id: String,
    objective: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdEquipe {
    team_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdPlan {
    plan_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CheminAgent {
    path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CheminNote {
    rel_path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EcritureNote {
    rel_path: String,
    content: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdProvider {
    provider_id: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ProviderOptionnel {
    #[serde(default)]
    provider_id: Option<String>,
}

/// Exécute une commande pour le compte d'un client distant.
pub async fn dispatch(h: &Harness, commande: &str, p: Value) -> Result<Value, String> {
    if !commande_autorisee(commande) {
        return Err(format!("commande non exposée à distance : {commande}"));
    }
    match commande {
        // --- Fournisseurs ---
        "providers_list" => {
            let mut out = Vec::new();
            for prov in h.registry.list_all().await {
                out.push(ProviderInfo {
                    id: prov.id().to_string(),
                    name: prov.name().to_string(),
                    models: prov.list_models().await.unwrap_or_default(),
                });
            }
            out.sort_by(|a, b| a.id.cmp(&b.id));
            valeur(out)
        }
        "provider_test" => {
            let a: IdProvider = params(p)?;
            let prov = h
                .registry
                .get(&a.provider_id)
                .await
                .ok_or_else(|| format!("provider inconnu : {}", a.provider_id))?;
            let res = prov.health_check().await;
            valeur(json!({
                "providerId": a.provider_id,
                "ok": res.is_ok(),
                "error": res.err().map(|e| e.to_string()),
            }))
        }
        "models_list" => {
            let a: ProviderOptionnel = params(p).unwrap_or_default();
            let mut models = Vec::new();
            match a.provider_id {
                Some(pid) => {
                    let prov = h
                        .registry
                        .get(&pid)
                        .await
                        .ok_or_else(|| format!("provider inconnu : {pid}"))?;
                    models = prov.list_models().await.map_err(|e| e.to_string())?;
                }
                None => {
                    for prov in h.registry.list_all().await {
                        if let Ok(m) = prov.list_models().await {
                            models.extend(m);
                        }
                    }
                }
            }
            valeur(models)
        }
        "llm_health_check" => {
            let resultats = h.registry.health_check_all().await;
            let mut out: Vec<Value> = resultats
                .into_iter()
                .map(|(id, res)| {
                    json!({
                        "providerId": id,
                        "ok": res.is_ok(),
                        "error": res.err().map(|e| e.to_string()),
                    })
                })
                .collect();
            out.sort_by(|a, b| a["providerId"].as_str().cmp(&b["providerId"].as_str()));
            valeur(out)
        }
        "runtimes_detect" => valeur(crate::discovery::scan(&[]).await),

        // --- Coffre et agents ---
        "agents_list" => valeur(h.vault.agents_list()?),
        "agent_read" => {
            let a: CheminAgent = params(p)?;
            valeur(h.vault.agent_read(&a.path)?)
        }
        "vault_list" => valeur(h.vault.list_notes()?),
        "vault_read" => {
            let a: CheminNote = params(p)?;
            valeur(h.vault.read_note(&a.rel_path)?)
        }
        "vault_write" => {
            let a: EcritureNote = params(p)?;
            valeur(h.vault.write_note(&a.rel_path, &a.content)?)
        }
        "vault_path" => valeur(h.vault.root().display().to_string()),

        // --- Sessions ---
        "sessions_list" => valeur(h.sessions.list()),
        "session_create" => {
            #[derive(Deserialize)]
            struct Enveloppe {
                payload: crate::commands::SessionCreatePayload,
            }
            let a: Enveloppe = params(p)?;
            if a.payload.agent.is_some() && a.payload.team.is_some() {
                return Err("une session est menée par un agent OU par une équipe".into());
            }
            valeur(
                h.sessions
                    .create(
                        a.payload.agent,
                        a.payload.team,
                        a.payload.provider,
                        a.payload.model,
                    )
                    .await?,
            )
        }
        "session_get" => {
            let a: IdSession = params(p)?;
            valeur(h.sessions.get(&a.session_id)?)
        }
        "session_rename" => {
            let a: RenommageSession = params(p)?;
            valeur(h.sessions.rename(&a.session_id, &a.title).await?)
        }
        "session_delete" => {
            let a: IdSession = params(p)?;
            h.sessions.delete(&a.session_id).await?;
            Ok(Value::Null)
        }
        "session_cancel" => {
            let a: IdSession = params(p)?;
            valeur(h.sessions.cancel(&a.session_id).await)
        }
        "session_send" => {
            let a: EnvoiSession = params(p)?;
            session_send_impl(
                &h.bus,
                &h.sessions,
                &h.pool,
                &h.vault,
                a.session_id,
                a.content,
                None,
            )
            .await?;
            Ok(Value::Null)
        }
        "session_export" => {
            let a: ExportSession = params(p)?;
            let session = h.sessions.get(&a.session_id)?;
            if session.messages.is_empty() {
                return Err("session vide : rien à exporter".into());
            }
            let chemin = crate::session::export_path(&session, &a.project);
            valeur(
                h.vault
                    .write_note(&chemin, &crate::session::to_markdown(&session))?,
            )
        }

        // --- Équipes ---
        "teams_list" => valeur(h.teams.list()),
        "team_save" => {
            #[derive(Deserialize)]
            struct Enveloppe {
                payload: crate::team::Team,
            }
            let a: Enveloppe = params(p)?;
            h.teams.save(&a.payload)?;
            valeur(a.payload)
        }
        "team_delete" => {
            let a: IdEquipe = params(p)?;
            h.teams.delete(&a.team_id)?;
            Ok(Value::Null)
        }
        "team_run" => {
            let a: ExecutionEquipe = params(p)?;
            team_run_impl(
                &h.bus,
                &h.sessions,
                &h.teams,
                &h.pool,
                &h.vault,
                a.session_id,
                a.objective,
            )
            .await?;
            Ok(Value::Null)
        }

        // --- Plans ---
        "plans_list" => valeur(h.plans.list()),
        "plan_save" => {
            #[derive(Deserialize)]
            struct Enveloppe {
                payload: crate::plan::Plan,
            }
            let a: Enveloppe = params(p)?;
            h.plans.save(&a.payload).await?;
            valeur(a.payload)
        }
        "plan_delete" => {
            let a: IdPlan = params(p)?;
            h.plans.delete(&a.plan_id).await?;
            Ok(Value::Null)
        }
        "plan_cancel" => {
            let a: IdPlan = params(p)?;
            valeur(h.plans.cancel(&a.plan_id).await)
        }
        "plan_run" => {
            let a: IdPlan = params(p)?;
            valeur(plan_run_impl(&h.bus, &h.plans, &h.pool, &h.vault, a.plan_id).await?)
        }

        autre => Err(format!("commande inconnue : {autre}")),
    }
}

// ===== Serveur =====

#[derive(Clone)]
struct EtatServeur {
    harness: Harness,
    jeton: Arc<String>,
}

/// Extrait le jeton d'une requête : en-tête `Authorization: Bearer …`, ou
/// paramètre `token` (les WebSockets du navigateur ne peuvent pas porter
/// d'en-tête personnalisé).
fn jeton_de(headers: &HeaderMap, query: &HashMap<String, String>) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|v| v.trim().to_string())
        .or_else(|| query.get("token").cloned())
}

async fn health() -> impl IntoResponse {
    // Volontairement non authentifiée et sans détail : elle sert au client à
    // savoir qu'un hôte répond, avant même d'avoir un jeton valide.
    Json(json!({ "status": "ok", "service": "obsiaui" }))
}

async fn rpc(
    AxumState(etat): AxumState<EtatServeur>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    Json(req): Json<RpcRequest>,
) -> Response {
    match jeton_de(&headers, &query) {
        Some(j) if jetons_egaux(&etat.jeton, &j) => {}
        _ => {
            warn!(commande = %req.command, "appel distant refusé : jeton invalide");
            return (
                StatusCode::UNAUTHORIZED,
                Json(RpcResponse::echec("jeton invalide".into())),
            )
                .into_response();
        }
    }
    match dispatch(&etat.harness, &req.command, req.params).await {
        Ok(v) => Json(RpcResponse::succes(v)).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(RpcResponse::echec(e))).into_response(),
    }
}

async fn ws(
    AxumState(etat): AxumState<EtatServeur>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    upgrade: WebSocketUpgrade,
) -> Response {
    match jeton_de(&headers, &query) {
        Some(j) if jetons_egaux(&etat.jeton, &j) => {}
        _ => {
            warn!("abonnement distant refusé : jeton invalide");
            return (StatusCode::UNAUTHORIZED, "jeton invalide").into_response();
        }
    }
    upgrade.on_upgrade(move |socket| pousser_evenements(socket, etat.harness.bus.clone()))
}

/// Pousse le flux d'événements vers un client connecté.
async fn pousser_evenements(mut socket: WebSocket, bus: Arc<EventBus>) {
    let mut rx = bus.subscribe();
    info!("client distant abonné au flux d'événements");
    loop {
        match rx.recv().await {
            Ok(ev) => {
                let Ok(texte) = serde_json::to_string(&ev) else {
                    continue;
                };
                if socket.send(Message::Text(texte.into())).await.is_err() {
                    break;
                }
            }
            // Client trop lent : on saute les événements perdus plutôt que de
            // fermer la connexion, la suite du flux reste utile.
            Err(tokio::sync::broadcast::error::RecvError::Lagged(perdus)) => {
                warn!(perdus, "client distant en retard sur le flux");
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
    info!("client distant déconnecté");
}

/// Serveur en cours d'exécution.
pub struct ServeurActif {
    pub addr: SocketAddr,
    arret: oneshot::Sender<()>,
}

/// État du daemon, partagé avec les commandes Tauri.
#[derive(Default)]
pub struct RemoteState {
    actif: Mutex<Option<ServeurActif>>,
}

impl RemoteState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adresse d'écoute, si le serveur tourne.
    pub async fn adresse(&self) -> Option<SocketAddr> {
        self.actif.lock().await.as_ref().map(|s| s.addr)
    }

    /// Démarre le serveur. Un serveur déjà actif est d'abord arrêté, pour
    /// qu'un changement d'adresse ou de jeton prenne effet sans redémarrage
    /// de l'application.
    pub async fn demarrer(
        &self,
        harness: Harness,
        bind: &str,
        jeton: &str,
    ) -> Result<SocketAddr, String> {
        let addr = valider_ecoute(bind, jeton)?;
        self.arreter().await;

        let etat = EtatServeur {
            harness,
            jeton: Arc::new(jeton.to_string()),
        };
        let app = Router::new()
            .route("/health", get(health))
            .route("/rpc", post(rpc))
            .route("/ws", get(ws))
            .with_state(etat);

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| format!("écoute sur {addr} impossible : {e}"))?;
        let reelle = listener
            .local_addr()
            .map_err(|e| format!("adresse locale illisible : {e}"))?;

        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            let serveur = axum::serve(listener, app).with_graceful_shutdown(async {
                let _ = rx.await;
            });
            if let Err(e) = serveur.await {
                warn!(%e, "serveur distant interrompu");
            }
        });

        if est_boucle(&reelle) {
            info!(%reelle, "serveur distant démarré (boucle locale)");
        } else {
            warn!(%reelle, "serveur distant exposé au réseau — jeton exigé");
        }
        *self.actif.lock().await = Some(ServeurActif {
            addr: reelle,
            arret: tx,
        });
        Ok(reelle)
    }

    /// Arrête le serveur. Sans effet s'il ne tourne pas.
    pub async fn arreter(&self) -> bool {
        match self.actif.lock().await.take() {
            Some(s) => {
                let _ = s.arret.send(());
                info!(addr = %s.addr, "serveur distant arrêté");
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== Jetons =====

    #[test]
    fn un_jeton_fait_128_bits_en_hexadecimal() {
        let j = generer_jeton();
        assert_eq!(j.len(), 32);
        assert!(j.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn deux_jetons_successifs_different() {
        assert_ne!(generer_jeton(), generer_jeton());
    }

    #[test]
    fn la_comparaison_reconnait_le_bon_jeton() {
        let j = generer_jeton();
        assert!(jetons_egaux(&j, &j.clone()));
    }

    #[test]
    fn la_comparaison_rejette_tout_le_reste() {
        let j = "a".repeat(32);
        assert!(!jetons_egaux(&j, ""));
        assert!(!jetons_egaux(&j, &"a".repeat(31)));
        assert!(!jetons_egaux(&j, &"a".repeat(33)));
        // Un préfixe correct ne doit pas passer : c'est exactement l'attaque
        // que la comparaison à temps constant vise à rendre inexploitable.
        assert!(!jetons_egaux(&j, &format!("{}b", "a".repeat(31))));
    }

    #[test]
    fn un_jeton_vide_ne_vaut_jamais_authentification() {
        // Sans cette garde, une config sans jeton laisserait tout passer.
        assert!(!jetons_egaux("", ""));
    }

    // ===== Écoute =====

    #[test]
    fn l_ecoute_par_defaut_est_locale() {
        let addr = valider_ecoute("", &"x".repeat(JETON_MIN)).unwrap();
        assert!(est_boucle(&addr));
        assert_eq!(addr.port(), PORT_DEFAUT);
    }

    #[test]
    fn une_adresse_sans_port_prend_le_port_conventionnel() {
        let addr = valider_ecoute("0.0.0.0", &"x".repeat(JETON_MIN)).unwrap();
        assert_eq!(addr.port(), PORT_DEFAUT);
        assert!(!est_boucle(&addr));
    }

    #[test]
    fn un_jeton_trop_court_est_refuse_meme_en_local() {
        // Sur une machine partagée, tout processus local atteint 127.0.0.1.
        let err = valider_ecoute("127.0.0.1:7420", "court").unwrap_err();
        assert!(err.contains("jeton"));
        assert!(valider_ecoute("127.0.0.1:7420", "").is_err());
    }

    #[test]
    fn une_adresse_invalide_est_refusee() {
        assert!(valider_ecoute("pas une adresse", &"x".repeat(JETON_MIN)).is_err());
    }

    #[test]
    fn une_ecoute_reseau_avec_jeton_solide_est_acceptee() {
        let addr = valider_ecoute("0.0.0.0:9000", &generer_jeton()).unwrap();
        assert_eq!(addr.port(), 9000);
        assert!(!est_boucle(&addr));
    }

    // ===== Liste blanche =====

    #[test]
    fn la_configuration_de_l_hote_n_est_pas_exposee() {
        // Détenir le jeton donne la main sur le travail, pas le droit de lire
        // les clés d'API, de rebinder l'écoute ni de changer le jeton.
        for interdite in [
            "config_set",
            "config_get",
            "remote_start",
            "remote_stop",
            "remote_token",
            "scan_local_models",
        ] {
            assert!(
                !commande_autorisee(interdite),
                "{interdite} ne doit pas être exposée"
            );
        }
    }

    #[test]
    fn les_commandes_de_travail_sont_exposees() {
        for permise in [
            "sessions_list",
            "session_send",
            "team_run",
            "plan_run",
            "vault_read",
            "runtimes_detect",
        ] {
            assert!(commande_autorisee(permise), "{permise} doit être exposée");
        }
    }

    // ===== Extraction du jeton =====

    #[test]
    fn le_jeton_se_lit_dans_l_en_tete_ou_la_requete() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer abc123".parse().unwrap());
        let vide = HashMap::new();
        assert_eq!(jeton_de(&headers, &vide).as_deref(), Some("abc123"));

        // Les WebSockets du navigateur ne peuvent pas porter d'en-tête.
        let mut query = HashMap::new();
        query.insert("token".to_string(), "depuis-url".to_string());
        assert_eq!(
            jeton_de(&HeaderMap::new(), &query).as_deref(),
            Some("depuis-url")
        );
    }

    #[test]
    fn l_en_tete_prime_sur_la_requete() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer entete".parse().unwrap());
        let mut query = HashMap::new();
        query.insert("token".to_string(), "url".to_string());
        assert_eq!(jeton_de(&headers, &query).as_deref(), Some("entete"));
    }

    #[test]
    fn un_en_tete_mal_forme_ne_donne_pas_de_jeton() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Basic abc".parse().unwrap());
        assert_eq!(jeton_de(&headers, &HashMap::new()), None);
    }

    // ===== Cycle de vie =====

    #[tokio::test]
    async fn arreter_un_serveur_au_repos_est_sans_effet() {
        let etat = RemoteState::new();
        assert!(!etat.arreter().await);
        assert!(etat.adresse().await.is_none());
    }

    // ===== Serveur réel =====

    fn harness_de_test(dir: &std::path::Path) -> Harness {
        use crate::llm::fallback::{PoolStrategy, ProviderPool};
        Harness {
            bus: Arc::new(EventBus::new()),
            sessions: Arc::new(SessionManager::new(dir.join("sessions"))),
            teams: Arc::new(TeamStore::new(dir.join("teams"))),
            plans: Arc::new(PlanManager::new(dir.join("plans"))),
            pool: Arc::new(ProviderPool::new(PoolStrategy::Fallback)),
            registry: Arc::new(ProviderRegistry::new()),
            // Coffre absent : les commandes de coffre échoueront proprement,
            // ce qui suffit pour éprouver le transport et l'authentification.
            vault: Arc::new(VaultState::unavailable("test".into())),
        }
    }

    async fn appel(
        client: &reqwest::Client,
        addr: SocketAddr,
        jeton: Option<&str>,
        commande: &str,
        params: Value,
    ) -> (u16, Value) {
        let mut req = client
            .post(format!("http://{addr}/rpc"))
            .json(&json!({ "command": commande, "params": params }));
        if let Some(j) = jeton {
            req = req.bearer_auth(j);
        }
        let resp = req.send().await.expect("requête RPC");
        let code = resp.status().as_u16();
        (code, resp.json().await.unwrap_or(Value::Null))
    }

    /// Éprouve le serveur de bout en bout : santé, authentification, liste
    /// blanche et un aller-retour utile.
    #[tokio::test]
    async fn le_serveur_repond_et_filtre_les_appels() {
        let dir = tempfile::tempdir().unwrap();
        let etat = RemoteState::new();
        let jeton = generer_jeton();
        // Port 0 : le système en attribue un libre, le test ne dépend pas
        // d'un port fixe qui pourrait déjà être pris.
        let addr = etat
            .demarrer(harness_de_test(dir.path()), "127.0.0.1:0", &jeton)
            .await
            .expect("démarrage du serveur");
        assert!(est_boucle(&addr));

        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        // La santé répond sans jeton : elle sert à trouver un hôte.
        let sante = client
            .get(format!("http://{addr}/health"))
            .send()
            .await
            .expect("santé");
        assert_eq!(sante.status().as_u16(), 200);
        let corps: Value = sante.json().await.unwrap();
        assert_eq!(corps["service"], "obsiaui");

        // Sans jeton : refusé.
        let (code, _) = appel(&client, addr, None, "sessions_list", json!({})).await;
        assert_eq!(code, 401);

        // Mauvais jeton : refusé.
        let (code, _) = appel(
            &client,
            addr,
            Some(&generer_jeton()),
            "sessions_list",
            json!({}),
        )
        .await;
        assert_eq!(code, 401);

        // Bon jeton : la commande passe.
        let (code, corps) = appel(&client, addr, Some(&jeton), "sessions_list", json!({})).await;
        assert_eq!(code, 200);
        assert_eq!(corps["ok"], true);
        assert_eq!(corps["result"], json!([]));

        // Un aller-retour utile : créer puis relire une session.
        let (code, corps) = appel(
            &client,
            addr,
            Some(&jeton),
            "session_create",
            json!({ "payload": { "model": "qwen3:8b" } }),
        )
        .await;
        assert_eq!(code, 200, "création refusée : {corps}");
        let id = corps["result"]["id"].as_str().unwrap().to_string();
        let (_, corps) = appel(&client, addr, Some(&jeton), "sessions_list", json!({})).await;
        assert_eq!(corps["result"][0]["id"], id);

        // Hors liste blanche : refusé même avec le bon jeton.
        let (code, corps) = appel(&client, addr, Some(&jeton), "config_set", json!({})).await;
        assert_eq!(code, 400);
        assert_eq!(corps["ok"], false);
        assert!(corps["error"].as_str().unwrap().contains("non exposée"));

        assert!(etat.arreter().await);
    }

    /// Le flux d'événements atteint un client abonné par WebSocket, et lui
    /// seulement s'il présente le jeton.
    #[tokio::test]
    async fn le_flux_d_evenements_atteint_un_client_authentifie() {
        use futures::StreamExt;
        use tokio_tungstenite::connect_async;

        let dir = tempfile::tempdir().unwrap();
        let etat = RemoteState::new();
        let jeton = generer_jeton();
        let harness = harness_de_test(dir.path());
        let bus = harness.bus.clone();
        let addr = etat
            .demarrer(harness, "127.0.0.1:0", &jeton)
            .await
            .expect("démarrage du serveur");

        // Sans jeton valide, la montée en WebSocket est refusée.
        assert!(
            connect_async(format!("ws://{addr}/ws?token={}", generer_jeton()))
                .await
                .is_err(),
            "un jeton invalide ne doit pas ouvrir le flux"
        );

        // Avec le bon jeton, le client reçoit le même flux que la fenêtre.
        let (mut socket, _) = connect_async(format!("ws://{addr}/ws?token={jeton}"))
            .await
            .expect("abonnement au flux");

        // L'abonnement est effectif avant d'émettre, sinon l'événement
        // partirait dans le vide et le test serait instable.
        for _ in 0..100 {
            if bus.receiver_count() > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        bus.emit(
            "session:token",
            json!({ "sessionId": "s1", "token": "salut" }),
        );

        let recu = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next())
            .await
            .expect("le flux doit livrer l'événement")
            .expect("message présent")
            .expect("message lisible");
        let texte = recu.into_text().expect("message texte");
        let ev: Value = serde_json::from_str(&texte).expect("événement JSON");
        assert_eq!(ev["name"], "session:token");
        assert_eq!(ev["payload"]["token"], "salut");

        assert!(etat.arreter().await);
    }
}
