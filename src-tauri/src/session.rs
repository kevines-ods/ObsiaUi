//! Sessions de conversation multiples et concurrentes.
//!
//! Une session = un fil de discussion persistant : son agent, son fournisseur,
//! son modèle, et l'historique des messages. Plusieurs sessions peuvent
//! streamer **en même temps** — d'où deux choix structurants :
//!
//! - **Un fichier JSON par session**, écrit de façon atomique (fichier
//!   temporaire puis `rename`). Deux sessions ne partagent aucun fichier ; une
//!   coupure de courant ne laisse jamais un JSON tronqué.
//! - **Les événements de flux portent l'identifiant de session** plutôt que de
//!   dériver un nom d'événement par session. L'interface s'abonne une seule
//!   fois et filtre : pas d'abonnements à gérer à chaque ouverture d'onglet.
//!
//! L'état des sessions vit dans les données applicatives, **jamais dans le
//! coffre** : celui-ci est en lecture seule (hors `brouillon/`) et n'a pas à
//! porter de l'état volatil. Une session jugée digne d'être gardée est
//! exportée en note Markdown dans `brouillon/`, d'où un humain la déplace par
//! patch revu (cf. `VAULT-CONTRACT.md` §2).

use crate::llm::provider::ChatMessage;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, RwLock};
use tracing::{info, warn};

/// Titre donné à une session tant qu'aucun message ne permet d'en déduire un.
pub const TITRE_PAR_DEFAUT: &str = "Nouvelle session";

/// Longueur maximale d'un titre dérivé d'un message.
const TITRE_MAX: usize = 60;

// ===== Modèle =====

/// Un message de l'historique.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    /// `user`, `assistant` ou `system`.
    pub role: String,
    pub content: String,
    /// Horodatage Unix en secondes.
    pub at: u64,
    /// Agent auteur du message. Renseigné pour les sessions d'équipe, où
    /// plusieurs agents parlent dans le même fil.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

impl SessionMessage {
    pub fn new(role: &str, content: impl Into<String>) -> Self {
        Self {
            role: role.to_string(),
            content: content.into(),
            at: now(),
            agent: None,
        }
    }

    pub fn with_agent(mut self, agent: Option<String>) -> Self {
        self.agent = agent;
        self
    }
}

/// Métadonnées d'une session — ce que liste l'interface, sans charger les
/// messages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub id: String,
    pub title: String,
    /// Agent du coffre pilotant la session (`IA/agents/<nom>.md`).
    pub agent: Option<String>,
    /// Fournisseur ciblé. `None` = repli automatique sur le pool.
    pub provider: Option<String>,
    pub model: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub message_count: usize,
}

/// Session complète : métadonnées + historique.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    #[serde(flatten)]
    pub meta: SessionMeta,
    #[serde(default)]
    pub messages: Vec<SessionMessage>,
}

impl Session {
    pub fn new(agent: Option<String>, provider: Option<String>, model: String) -> Self {
        let at = now();
        Self {
            meta: SessionMeta {
                id: uuid::Uuid::new_v4().to_string(),
                title: TITRE_PAR_DEFAUT.to_string(),
                agent,
                provider,
                model,
                created_at: at,
                updated_at: at,
                message_count: 0,
            },
            messages: Vec::new(),
        }
    }

    /// Ajoute un message, met à jour les compteurs et, si le titre est encore
    /// celui par défaut, le dérive du premier message de l'utilisateur.
    pub fn push(&mut self, message: SessionMessage) {
        if self.meta.title == TITRE_PAR_DEFAUT && message.role == "user" {
            self.meta.title = derive_title(&message.content);
        }
        self.meta.updated_at = message.at;
        self.messages.push(message);
        self.meta.message_count = self.messages.len();
    }

    /// Construit la requête envoyée au modèle : prompt système de l'agent
    /// d'abord (s'il y en a un), puis l'historique.
    ///
    /// Les messages `system` déjà présents dans l'historique sont ignorés :
    /// le prompt de l'agent est la seule source, et le coffre peut l'avoir
    /// modifié depuis le début de la conversation.
    pub fn to_chat_messages(&self, system_prompt: Option<&str>) -> Vec<ChatMessage> {
        let mut out = Vec::with_capacity(self.messages.len() + 1);
        if let Some(prompt) = system_prompt.map(str::trim).filter(|p| !p.is_empty()) {
            out.push(ChatMessage {
                role: "system".to_string(),
                content: prompt.to_string(),
                name: None,
            });
        }
        for m in &self.messages {
            if m.role == "system" {
                continue;
            }
            out.push(ChatMessage {
                role: m.role.clone(),
                content: m.content.clone(),
                // `name` porte l'agent : utile aux sessions d'équipe, où le
                // modèle doit distinguer qui a dit quoi.
                name: m.agent.clone(),
            });
        }
        out
    }
}

/// Titre dérivé du premier message : première ligne non vide, tronquée sur
/// une frontière de mot pour éviter de couper au milieu.
pub fn derive_title(content: &str) -> String {
    let ligne = content
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if ligne.is_empty() {
        return TITRE_PAR_DEFAUT.to_string();
    }
    if ligne.chars().count() <= TITRE_MAX {
        return ligne.to_string();
    }
    let tronque: String = ligne.chars().take(TITRE_MAX).collect();
    match tronque.rsplit_once(' ') {
        // On ne recule jusqu'au dernier espace que s'il reste un titre
        // substantiel — sinon un premier « mot » très long donnerait un titre
        // vide.
        Some((debut, _)) if debut.chars().count() >= TITRE_MAX / 3 => format!("{debut}…"),
        _ => format!("{tronque}…"),
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ===== Stockage =====

/// Stockage sur disque : un fichier JSON par session.
pub struct SessionStore {
    dir: PathBuf,
}

impl SessionStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Chemin du fichier d'une session.
    ///
    /// L'identifiant est validé même s'il est engendré en interne : il
    /// transite par l'IPC, donc par du code que l'on ne contrôle pas. Sans
    /// cette barrière, un `id` valant `../../.ssh/config` sortirait du
    /// dossier de stockage.
    fn path_for(&self, id: &str) -> Result<PathBuf, String> {
        if !est_id_valide(id) {
            return Err(format!("identifiant de session invalide : {id}"));
        }
        Ok(self.dir.join(format!("{id}.json")))
    }

    /// Écrit la session de façon atomique : fichier temporaire puis `rename`.
    /// Un `rename` sur le même système de fichiers est atomique — le lecteur
    /// voit l'ancienne version ou la nouvelle, jamais un JSON à moitié écrit.
    pub fn save(&self, session: &Session) -> Result<(), String> {
        let path = self.path_for(&session.meta.id)?;
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| format!("création de {} impossible: {e}", self.dir.display()))?;
        let raw = serde_json::to_string_pretty(session)
            .map_err(|e| format!("sérialisation de la session: {e}"))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, raw).map_err(|e| format!("écriture de {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &path).map_err(|e| {
            // Le temporaire ne doit pas rester derrière un échec de rename.
            let _ = std::fs::remove_file(&tmp);
            format!("remplacement de {}: {e}", path.display())
        })?;
        Ok(())
    }

    pub fn load(&self, id: &str) -> Result<Session, String> {
        let path = self.path_for(id)?;
        let raw =
            std::fs::read_to_string(&path).map_err(|e| format!("session {id} illisible: {e}"))?;
        serde_json::from_str(&raw).map_err(|e| format!("session {id} corrompue: {e}"))
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        let path = self.path_for(id)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            // Supprimer deux fois n'est pas une erreur pour l'appelant.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("suppression de {}: {e}", path.display())),
        }
    }

    /// Métadonnées de toutes les sessions, la plus récemment modifiée en tête.
    ///
    /// Un fichier illisible ou corrompu est ignoré avec un avertissement :
    /// une session cassée ne doit pas rendre la liste entière inutilisable.
    pub fn list(&self) -> Vec<SessionMeta> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut metas: Vec<SessionMeta> = entries
            .flatten()
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
            .filter_map(|e| match std::fs::read_to_string(e.path()) {
                Ok(raw) => match serde_json::from_str::<Session>(&raw) {
                    Ok(s) => Some(s.meta),
                    Err(err) => {
                        warn!(fichier = %e.path().display(), %err, "session illisible, ignorée");
                        None
                    }
                },
                Err(err) => {
                    warn!(fichier = %e.path().display(), %err, "session illisible, ignorée");
                    None
                }
            })
            .collect();
        metas.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
        metas
    }
}

/// Un identifiant de session est un UUID : minuscules, chiffres et tirets.
pub fn est_id_valide(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

// ===== Gestionnaire =====

/// Gestionnaire de sessions : stockage + annulations en cours.
pub struct SessionManager {
    store: SessionStore,
    /// Sérialise les cycles lecture-modification-écriture. Les sessions ont
    /// chacune leur fichier, mais renommer une session pendant qu'elle répond
    /// lit et réécrit le même fichier : sans ce verrou, l'une des deux
    /// écritures serait perdue.
    ecriture: Mutex<()>,
    /// Drapeaux d'annulation, par identifiant de session.
    annulations: RwLock<std::collections::HashMap<String, Arc<AtomicBool>>>,
}

impl SessionManager {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        let store = SessionStore::new(dir);
        info!(dossier = %store.dir().display(), "stockage des sessions");
        Self {
            store,
            ecriture: Mutex::new(()),
            annulations: RwLock::new(std::collections::HashMap::new()),
        }
    }

    pub fn list(&self) -> Vec<SessionMeta> {
        self.store.list()
    }

    pub fn get(&self, id: &str) -> Result<Session, String> {
        self.store.load(id)
    }

    pub async fn create(
        &self,
        agent: Option<String>,
        provider: Option<String>,
        model: String,
    ) -> Result<SessionMeta, String> {
        let session = Session::new(agent, provider, model);
        let _guard = self.ecriture.lock().await;
        self.store.save(&session)?;
        info!(id = %session.meta.id, "session créée");
        Ok(session.meta)
    }

    pub async fn delete(&self, id: &str) -> Result<(), String> {
        self.cancel(id).await;
        let _guard = self.ecriture.lock().await;
        self.annulations.write().await.remove(id);
        self.store.delete(id)
    }

    /// Applique une transformation à une session puis la réécrit.
    pub async fn update<F>(&self, id: &str, f: F) -> Result<SessionMeta, String>
    where
        F: FnOnce(&mut Session),
    {
        let _guard = self.ecriture.lock().await;
        let mut session = self.store.load(id)?;
        f(&mut session);
        self.store.save(&session)?;
        Ok(session.meta)
    }

    pub async fn rename(&self, id: &str, title: &str) -> Result<SessionMeta, String> {
        let title = title.trim().to_string();
        if title.is_empty() {
            return Err("titre vide refusé".into());
        }
        self.update(id, |s| {
            s.meta.title = title;
            s.meta.updated_at = now();
        })
        .await
    }

    pub async fn push_message(
        &self,
        id: &str,
        message: SessionMessage,
    ) -> Result<SessionMeta, String> {
        self.update(id, |s| s.push(message)).await
    }

    /// Arme un drapeau d'annulation pour une session et renvoie sa poignée.
    /// Remplace le drapeau précédent : un nouvel envoi ne doit pas hériter
    /// d'une annulation demandée pour le tour d'avant.
    pub async fn begin(&self, id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.annulations
            .write()
            .await
            .insert(id.to_string(), flag.clone());
        flag
    }

    /// Demande l'annulation du tour en cours. Sans effet si rien ne tourne.
    pub async fn cancel(&self, id: &str) -> bool {
        match self.annulations.read().await.get(id) {
            Some(flag) => {
                flag.store(true, Ordering::SeqCst);
                info!(id, "annulation demandée");
                true
            }
            None => false,
        }
    }

    /// Libère le drapeau à la fin d'un tour.
    pub async fn finish(&self, id: &str) {
        self.annulations.write().await.remove(id);
    }
}

// ===== Export vers le coffre =====

/// Rend une session en note Markdown destinée au coffre.
///
/// La note ne porte pas de frontmatter : le contrat n'en exige que pour les
/// fichiers agent, skill et contrat (§5). C'est une note de mémoire.
pub fn to_markdown(session: &Session) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", session.meta.title));
    out.push_str(&format!(
        "> Session `{}` — {} message(s), modèle `{}`",
        session.meta.id, session.meta.message_count, session.meta.model
    ));
    if let Some(agent) = &session.meta.agent {
        out.push_str(&format!(", agent `{agent}`"));
    }
    out.push_str(&format!(
        ".\n> Ouverte le {}, dernière activité le {}.\n\n",
        date_iso(session.meta.created_at),
        date_iso(session.meta.updated_at)
    ));

    for m in &session.messages {
        let qui = match m.role.as_str() {
            "user" => "Question".to_string(),
            "assistant" => match &m.agent {
                Some(a) => format!("Réponse — {a}"),
                None => "Réponse".to_string(),
            },
            autre => autre.to_string(),
        };
        out.push_str(&format!("## {qui}\n\n{}\n\n", m.content.trim()));
    }
    out
}

/// Chemin de brouillon d'un export.
///
/// Il reproduit la structure de la mémoire du coffre
/// (`mémoire/<agent>/<projet>/AAAA-MM-JJ-titre.md`) sous `brouillon/`, seule
/// zone écrivable : un humain n'a plus qu'à déplacer l'arborescence lors de la
/// revue, sans réorganiser quoi que ce soit.
pub fn export_path(session: &Session, projet: &str) -> String {
    let agent = session.meta.agent.as_deref().unwrap_or("sans-agent");
    format!(
        "brouillon/mémoire/{}/{}/{}-{}.md",
        slug(agent),
        slug(projet),
        date_iso(session.meta.updated_at),
        slug(&session.meta.title)
    )
}

/// Normalise un texte en segment de nom de fichier : minuscules, tirets, pas
/// d'espaces. Les accents sont conservés — le contrat les autorise (§5).
pub fn slug(texte: &str) -> String {
    let mut out = String::new();
    let mut tiret_en_attente = false;
    for c in texte.chars() {
        if c.is_alphanumeric() {
            if tiret_en_attente && !out.is_empty() {
                out.push('-');
            }
            tiret_en_attente = false;
            out.extend(c.to_lowercase());
        } else {
            // Toute suite de séparateurs se réduit à un tiret unique.
            tiret_en_attente = true;
        }
    }
    if out.is_empty() {
        "sans-titre".to_string()
    } else {
        out.chars().take(60).collect()
    }
}

/// Date `AAAA-MM-JJ` (UTC) depuis un horodatage Unix, sans dépendance externe.
///
/// Algorithme civil-from-days de Howard Hinnant : l'année est décalée pour
/// faire commencer l'ère en mars, ce qui place le 29 février en fin de cycle
/// et supprime tout cas particulier d'année bissextile.
pub fn date_iso(secondes: u64) -> String {
    let jours = (secondes / 86_400) as i64;
    let z = jours + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_test() -> Session {
        Session::new(
            Some("assistant".into()),
            Some("ollama".into()),
            "qwen3:8b".into(),
        )
    }

    // ===== Titres =====

    #[test]
    fn le_titre_vient_du_premier_message_utilisateur() {
        let mut s = session_test();
        assert_eq!(s.meta.title, TITRE_PAR_DEFAUT);
        s.push(SessionMessage::new("user", "Comment détecter llama.cpp ?"));
        assert_eq!(s.meta.title, "Comment détecter llama.cpp ?");
    }

    #[test]
    fn le_titre_ne_change_plus_ensuite() {
        let mut s = session_test();
        s.push(SessionMessage::new("user", "premier"));
        s.push(SessionMessage::new("assistant", "réponse"));
        s.push(SessionMessage::new("user", "second"));
        assert_eq!(s.meta.title, "premier");
    }

    #[test]
    fn une_reponse_seule_ne_donne_pas_le_titre() {
        // Une session d'équipe peut commencer par un message d'agent ; le
        // titre doit rester en attente d'une vraie question.
        let mut s = session_test();
        s.push(SessionMessage::new("assistant", "bonjour"));
        assert_eq!(s.meta.title, TITRE_PAR_DEFAUT);
    }

    #[test]
    fn un_titre_long_est_tronque_sur_un_mot() {
        let long = "détecter les moteurs locaux puis recâbler les fournisseurs sur les adresses joignables";
        let titre = derive_title(long);
        assert!(titre.ends_with('…'));
        assert!(titre.chars().count() <= TITRE_MAX + 1);
        assert!(!titre.contains("  "));
        // La troncature tombe sur une frontière de mot.
        assert!(long.starts_with(titre.trim_end_matches('…')));
    }

    #[test]
    fn un_premier_mot_interminable_ne_donne_pas_un_titre_vide() {
        let titre = derive_title(&"a".repeat(200));
        assert!(titre.chars().count() > 1, "titre obtenu : {titre:?}");
    }

    #[test]
    fn un_message_vide_garde_le_titre_par_defaut() {
        assert_eq!(derive_title("   \n\n  "), TITRE_PAR_DEFAUT);
    }

    #[test]
    fn le_titre_saute_les_lignes_vides_de_tete() {
        assert_eq!(
            derive_title("\n\n  Vraie question\nsuite"),
            "Vraie question"
        );
    }

    // ===== Construction de la requête =====

    #[test]
    fn le_prompt_systeme_est_place_en_tete() {
        let mut s = session_test();
        s.push(SessionMessage::new("user", "salut"));
        let msgs = s.to_chat_messages(Some("Tu es assistant."));
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[0].content, "Tu es assistant.");
        assert_eq!(msgs[1].role, "user");
    }

    #[test]
    fn un_prompt_vide_n_est_pas_envoye() {
        let mut s = session_test();
        s.push(SessionMessage::new("user", "salut"));
        assert_eq!(s.to_chat_messages(Some("   ")).len(), 1);
        assert_eq!(s.to_chat_messages(None).len(), 1);
    }

    #[test]
    fn les_anciens_messages_systeme_sont_ecartes() {
        // Sinon un changement de prompt dans le coffre laisserait l'ancien
        // prompt actif pour toujours, empilé avec le nouveau.
        let mut s = session_test();
        s.push(SessionMessage::new("system", "vieux prompt"));
        s.push(SessionMessage::new("user", "salut"));
        let msgs = s.to_chat_messages(Some("nouveau prompt"));
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].content, "nouveau prompt");
        assert_eq!(msgs[1].role, "user");
    }

    #[test]
    fn l_agent_auteur_est_transmis_comme_nom() {
        let mut s = session_test();
        s.push(SessionMessage::new("assistant", "vu").with_agent(Some("relecteur".into())));
        let msgs = s.to_chat_messages(None);
        assert_eq!(msgs[0].name.as_deref(), Some("relecteur"));
    }

    // ===== Stockage =====

    fn store_temporaire() -> (tempfile::TempDir, SessionStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(dir.path().join("sessions"));
        (dir, store)
    }

    #[test]
    fn aller_retour_sur_disque() {
        let (_d, store) = store_temporaire();
        let mut s = session_test();
        s.push(SessionMessage::new("user", "bonjour"));
        store.save(&s).unwrap();
        let relu = store.load(&s.meta.id).unwrap();
        assert_eq!(relu, s);
    }

    #[test]
    fn aucun_temporaire_ne_subsiste_apres_ecriture() {
        let (_d, store) = store_temporaire();
        let s = session_test();
        store.save(&s).unwrap();
        let restes: Vec<_> = std::fs::read_dir(store.dir())
            .unwrap()
            .flatten()
            .filter(|e| e.path().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(restes.is_empty(), "un fichier temporaire est resté");
    }

    #[test]
    fn un_identifiant_hostile_est_refuse() {
        // L'identifiant traverse l'IPC : sans validation, il sortirait du
        // dossier de stockage.
        let (_d, store) = store_temporaire();
        for mauvais in ["../evasion", "/etc/passwd", "a/b", "", "MAJUSCULE", "a b"] {
            assert!(
                store.load(mauvais).is_err(),
                "l'identifiant {mauvais:?} aurait dû être refusé"
            );
        }
        assert!(est_id_valide("3f2a1b8c-0000-4000-8000-000000000000"));
    }

    #[test]
    fn la_liste_place_la_plus_recente_en_tete() {
        let (_d, store) = store_temporaire();
        let mut ancienne = session_test();
        ancienne.meta.updated_at = 1_000;
        let mut recente = session_test();
        recente.meta.updated_at = 2_000;
        store.save(&ancienne).unwrap();
        store.save(&recente).unwrap();
        let liste = store.list();
        assert_eq!(liste.len(), 2);
        assert_eq!(liste[0].id, recente.meta.id);
    }

    #[test]
    fn une_session_corrompue_n_empeche_pas_de_lister_les_autres() {
        let (_d, store) = store_temporaire();
        let saine = session_test();
        store.save(&saine).unwrap();
        std::fs::write(store.dir().join("cassee.json"), b"{ pas du json").unwrap();
        let liste = store.list();
        assert_eq!(liste.len(), 1);
        assert_eq!(liste[0].id, saine.meta.id);
    }

    #[test]
    fn lister_un_dossier_absent_ne_panique_pas() {
        let store = SessionStore::new("/inexistant/vraiment");
        assert!(store.list().is_empty());
    }

    #[test]
    fn supprimer_deux_fois_reste_un_succes() {
        let (_d, store) = store_temporaire();
        let s = session_test();
        store.save(&s).unwrap();
        store.delete(&s.meta.id).unwrap();
        store.delete(&s.meta.id).unwrap();
        assert!(store.load(&s.meta.id).is_err());
    }

    // ===== Gestionnaire =====

    #[tokio::test]
    async fn creer_puis_relire_une_session() {
        let dir = tempfile::tempdir().unwrap();
        let m = SessionManager::new(dir.path());
        let meta = m
            .create(Some("assistant".into()), None, "qwen3:8b".into())
            .await
            .unwrap();
        assert_eq!(m.list().len(), 1);
        assert_eq!(m.get(&meta.id).unwrap().meta.model, "qwen3:8b");
    }

    #[tokio::test]
    async fn les_sessions_sont_independantes() {
        let dir = tempfile::tempdir().unwrap();
        let m = SessionManager::new(dir.path());
        let a = m.create(None, None, "m1".into()).await.unwrap();
        let b = m.create(None, None, "m2".into()).await.unwrap();
        m.push_message(&a.id, SessionMessage::new("user", "dans a"))
            .await
            .unwrap();
        assert_eq!(m.get(&a.id).unwrap().messages.len(), 1);
        assert_eq!(m.get(&b.id).unwrap().messages.len(), 0);
    }

    #[tokio::test]
    async fn renommer_refuse_un_titre_vide() {
        let dir = tempfile::tempdir().unwrap();
        let m = SessionManager::new(dir.path());
        let s = m.create(None, None, "m".into()).await.unwrap();
        assert!(m.rename(&s.id, "  ").await.is_err());
        assert_eq!(m.rename(&s.id, " Revue ").await.unwrap().title, "Revue");
    }

    #[tokio::test]
    async fn l_annulation_leve_le_drapeau_de_la_bonne_session() {
        let dir = tempfile::tempdir().unwrap();
        let m = SessionManager::new(dir.path());
        let a = m.begin("aaa").await;
        let b = m.begin("bbb").await;
        assert!(m.cancel("aaa").await);
        assert!(a.load(Ordering::SeqCst));
        assert!(!b.load(Ordering::SeqCst), "bbb ne devait pas être annulée");
    }

    #[tokio::test]
    async fn annuler_une_session_au_repos_est_sans_effet() {
        let dir = tempfile::tempdir().unwrap();
        let m = SessionManager::new(dir.path());
        assert!(!m.cancel("inconnue").await);
    }

    #[tokio::test]
    async fn un_nouveau_tour_n_herite_pas_de_l_annulation_precedente() {
        let dir = tempfile::tempdir().unwrap();
        let m = SessionManager::new(dir.path());
        m.begin("aaa").await;
        m.cancel("aaa").await;
        let neuf = m.begin("aaa").await;
        assert!(!neuf.load(Ordering::SeqCst));
    }

    // ===== Export =====

    #[test]
    fn l_export_reproduit_la_structure_de_la_memoire() {
        let mut s = session_test();
        s.meta.updated_at = 1_756_512_000; // 2025-08-30
        s.meta.title = "Détection des moteurs".into();
        let chemin = export_path(&s, "Harness multi-agent");
        assert_eq!(
            chemin,
            "brouillon/mémoire/assistant/harness-multi-agent/2025-08-30-détection-des-moteurs.md"
        );
        // Zone écrivable du coffre : toute autre racine serait refusée.
        assert!(chemin.starts_with("brouillon/"));
        assert!(chemin.ends_with(".md"));
    }

    #[test]
    fn l_export_sans_agent_reste_rangeable() {
        let mut s = Session::new(None, None, "m".into());
        s.meta.title = "sans agent".into();
        assert!(export_path(&s, "projet").contains("/sans-agent/"));
    }

    #[test]
    fn le_markdown_contient_les_echanges() {
        let mut s = session_test();
        s.push(SessionMessage::new("user", "Quelle heure ?"));
        s.push(SessionMessage::new("assistant", "Midi."));
        let md = to_markdown(&s);
        assert!(md.starts_with("# Quelle heure ?"));
        assert!(md.contains("## Question\n\nQuelle heure ?"));
        assert!(md.contains("## Réponse\n\nMidi."));
        assert!(md.contains("`qwen3:8b`"));
    }

    #[test]
    fn le_markdown_nomme_l_agent_qui_repond() {
        let mut s = session_test();
        s.push(SessionMessage::new("assistant", "ok").with_agent(Some("relecteur".into())));
        assert!(to_markdown(&s).contains("## Réponse — relecteur"));
    }

    // ===== Slug et date =====

    #[test]
    fn le_slug_ne_produit_ni_espace_ni_double_tiret() {
        assert_eq!(
            slug("Détection  des --- moteurs !"),
            "détection-des-moteurs"
        );
        assert_eq!(slug("  "), "sans-titre");
        assert_eq!(slug("Ollama & llama.cpp"), "ollama-llama-cpp");
    }

    #[test]
    fn le_slug_ne_commence_ni_ne_finit_par_un_tiret() {
        let s = slug("!! bord !!");
        assert!(!s.starts_with('-') && !s.ends_with('-'), "slug: {s}");
    }

    #[test]
    fn la_date_est_correcte_y_compris_sur_les_bissextiles() {
        assert_eq!(date_iso(0), "1970-01-01");
        assert_eq!(date_iso(1_756_512_000), "2025-08-30");
        // 2024-02-29 : le cas que les conversions maison ratent.
        assert_eq!(date_iso(1_709_164_800), "2024-02-29");
        // 2000-03-01 : lendemain du 29 février d'une année séculaire bissextile.
        assert_eq!(date_iso(951_868_800), "2000-03-01");
        // 1900 n'était pas bissextile ; on vérifie le 2100-03-01 côté futur.
        assert_eq!(date_iso(4_107_542_400), "2100-03-01");
    }
}
