//! Plans : un objectif découpé en étapes assignées à des agents.
//!
//! Un plan diffère d'une équipe. Une équipe discute ; un plan **exécute** une
//! structure connue d'avance : chaque étape a son agent, son modèle, et ses
//! dépendances. D'où trois propriétés que l'orchestration d'équipe n'offre
//! pas :
//!
//! - **Parallélisme réel.** Les étapes dont les dépendances sont satisfaites
//!   n'ont aucune raison de s'attendre : elles partent ensemble.
//! - **Contexte borné.** Une étape reçoit l'objectif et le résultat de ses
//!   dépendances — pas tout l'historique. Le prompt reste court même sur un
//!   plan long, et deux branches indépendantes ne se polluent pas.
//! - **Reprise.** Les statuts sont persistés : un plan interrompu redémarre
//!   là où il s'est arrêté au lieu de tout refaire.
//!
//! Un échec ne fait pas tomber le plan entier : seules les étapes qui en
//! dépendent sont écartées, les branches indépendantes continuent.

use crate::store::JsonStore;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Nombre maximal d'étapes par plan : garde-fou de coût et de lisibilité.
pub const MAX_ETAPES: usize = 40;

// ===== Modèle =====

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepStatus {
    /// En attente que ses dépendances soient satisfaites.
    Pending,
    Running,
    Done,
    Failed,
    /// Écartée : une dépendance a échoué ou été écartée.
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlanStatus {
    Draft,
    Running,
    /// Toutes les étapes ont abouti.
    Done,
    /// Terminé, mais au moins une étape a échoué ou été écartée.
    Incomplete,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    /// Identifiant court, unique dans le plan (`s1`, `redaction`…). Sert de
    /// cible aux dépendances.
    pub id: String,
    pub title: String,
    /// Consigne confiée à l'agent pour cette étape.
    pub instruction: String,
    /// Agent du coffre (`IA/agents/<nom>.md`).
    pub agent: String,
    #[serde(default)]
    pub provider: Option<String>,
    pub model: String,
    /// Étapes dont le résultat est nécessaire.
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default = "statut_initial")]
    pub status: StepStatus,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub started_at: Option<u64>,
    #[serde(default)]
    pub finished_at: Option<u64>,
}

fn statut_initial() -> StepStatus {
    StepStatus::Pending
}

impl PlanStep {
    /// Étape neuve, en attente.
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        instruction: impl Into<String>,
        agent: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            instruction: instruction.into(),
            agent: agent.into(),
            provider: None,
            model: model.into(),
            depends_on: Vec::new(),
            status: StepStatus::Pending,
            result: None,
            error: None,
            started_at: None,
            finished_at: None,
        }
    }

    pub fn depending_on(mut self, deps: &[&str]) -> Self {
        self.depends_on = deps.iter().map(|d| d.to_string()).collect();
        self
    }

    /// Une étape est close quand plus rien ne peut la faire avancer.
    pub fn est_close(&self) -> bool {
        matches!(
            self.status,
            StepStatus::Done | StepStatus::Failed | StepStatus::Skipped
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub id: String,
    pub title: String,
    pub objective: String,
    pub steps: Vec<PlanStep>,
    pub status: PlanStatus,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Plan {
    pub fn new(title: String, objective: String, steps: Vec<PlanStep>) -> Self {
        let at = now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            title,
            objective,
            steps,
            status: PlanStatus::Draft,
            created_at: at,
            updated_at: at,
        }
    }

    pub fn step(&self, id: &str) -> Option<&PlanStep> {
        self.steps.iter().find(|s| s.id == id)
    }

    fn step_mut(&mut self, id: &str) -> Option<&mut PlanStep> {
        self.steps.iter_mut().find(|s| s.id == id)
    }

    /// Vérifie qu'un plan est exécutable.
    ///
    /// Le contrôle de cycle est le point important : un cycle ne se voit pas à
    /// la lecture d'un plan de dix étapes, et à l'exécution il ne produirait
    /// pas d'erreur mais un blocage — aucune étape ne deviendrait jamais
    /// prête, le plan resterait « en cours » sans rien faire.
    pub fn validate(&self) -> Result<(), String> {
        if self.title.trim().is_empty() {
            return Err("le titre du plan est requis".into());
        }
        if self.objective.trim().is_empty() {
            return Err("l'objectif du plan est requis".into());
        }
        if self.steps.is_empty() {
            return Err("un plan compte au moins une étape".into());
        }
        if self.steps.len() > MAX_ETAPES {
            return Err(format!("un plan ne dépasse pas {MAX_ETAPES} étapes"));
        }

        let mut vus = HashSet::new();
        for s in &self.steps {
            if s.id.trim().is_empty() {
                return Err("chaque étape a un identifiant".into());
            }
            if !vus.insert(s.id.as_str()) {
                return Err(format!("l'étape « {} » est définie deux fois", s.id));
            }
            if s.title.trim().is_empty() || s.instruction.trim().is_empty() {
                return Err(format!(
                    "l'étape « {} » manque de titre ou de consigne",
                    s.id
                ));
            }
            if s.agent.trim().is_empty() || s.model.trim().is_empty() {
                return Err(format!("l'étape « {} » manque d'agent ou de modèle", s.id));
            }
        }
        for s in &self.steps {
            for d in &s.depends_on {
                if d == &s.id {
                    return Err(format!("l'étape « {} » dépend d'elle-même", s.id));
                }
                if !vus.contains(d.as_str()) {
                    return Err(format!(
                        "l'étape « {} » dépend de « {d} », qui n'existe pas",
                        s.id
                    ));
                }
            }
        }
        self.detecter_cycle()
    }

    /// Tri topologique (Kahn) : s'il reste des étapes non ordonnées, elles
    /// forment un cycle.
    fn detecter_cycle(&self) -> Result<(), String> {
        let mut restants: HashMap<&str, usize> = self
            .steps
            .iter()
            .map(|s| (s.id.as_str(), s.depends_on.len()))
            .collect();
        let mut dependants: HashMap<&str, Vec<&str>> = HashMap::new();
        for s in &self.steps {
            for d in &s.depends_on {
                dependants
                    .entry(d.as_str())
                    .or_default()
                    .push(s.id.as_str());
            }
        }
        let mut file: VecDeque<&str> = restants
            .iter()
            .filter(|(_, n)| **n == 0)
            .map(|(id, _)| *id)
            .collect();
        let mut ordonnes = 0usize;
        while let Some(id) = file.pop_front() {
            ordonnes += 1;
            for suivant in dependants.get(id).into_iter().flatten() {
                if let Some(n) = restants.get_mut(*suivant) {
                    *n -= 1;
                    if *n == 0 {
                        file.push_back(suivant);
                    }
                }
            }
        }
        if ordonnes != self.steps.len() {
            let bloquees: Vec<&str> = self
                .steps
                .iter()
                .map(|s| s.id.as_str())
                .filter(|id| restants.get(id).is_some_and(|n| *n > 0))
                .collect();
            return Err(format!(
                "dépendances circulaires entre les étapes : {}",
                bloquees.join(", ")
            ));
        }
        Ok(())
    }

    /// Étapes exécutables maintenant : en attente, toutes dépendances faites.
    ///
    /// Elles peuvent partir **en parallèle** — c'est tout l'intérêt d'un plan
    /// par rapport à une suite d'échanges.
    pub fn ready_steps(&self) -> Vec<&PlanStep> {
        self.steps
            .iter()
            .filter(|s| s.status == StepStatus::Pending)
            .filter(|s| {
                s.depends_on.iter().all(|d| {
                    self.step(d)
                        .is_some_and(|dep| dep.status == StepStatus::Done)
                })
            })
            .collect()
    }

    /// Vrai quand plus aucune étape ne peut avancer.
    pub fn est_termine(&self) -> bool {
        self.steps.iter().all(|s| s.est_close())
            || (self.ready_steps().is_empty()
                && !self.steps.iter().any(|s| s.status == StepStatus::Running))
    }

    pub fn mark_running(&mut self, id: &str) {
        if let Some(s) = self.step_mut(id) {
            s.status = StepStatus::Running;
            s.started_at = Some(now());
        }
        self.updated_at = now();
    }

    pub fn mark_done(&mut self, id: &str, resultat: String) {
        if let Some(s) = self.step_mut(id) {
            s.status = StepStatus::Done;
            s.result = Some(resultat);
            s.error = None;
            s.finished_at = Some(now());
        }
        self.updated_at = now();
    }

    /// Marque une étape en échec et **écarte celles qui en dépendent**,
    /// directement ou non.
    ///
    /// Sans cette propagation, les dépendantes resteraient éternellement en
    /// attente d'une dépendance qui n'aboutira jamais, et le plan paraîtrait
    /// en cours sans plus rien faire.
    pub fn mark_failed(&mut self, id: &str, erreur: String) {
        if let Some(s) = self.step_mut(id) {
            s.status = StepStatus::Failed;
            s.error = Some(erreur);
            s.finished_at = Some(now());
        }
        self.propager_abandon(id);
        self.updated_at = now();
    }

    fn propager_abandon(&mut self, origine: &str) {
        let mut perdues: HashSet<String> = HashSet::new();
        perdues.insert(origine.to_string());
        // Point fixe : une étape écartée en écarte d'autres à son tour.
        loop {
            let mut ajout = false;
            let a_ecarter: Vec<String> = self
                .steps
                .iter()
                .filter(|s| !s.est_close())
                .filter(|s| s.depends_on.iter().any(|d| perdues.contains(d)))
                .map(|s| s.id.clone())
                .collect();
            for id in a_ecarter {
                if perdues.insert(id.clone()) {
                    ajout = true;
                    if let Some(s) = self.step_mut(&id) {
                        s.status = StepStatus::Skipped;
                        s.error = Some(format!("dépendance « {origine} » non aboutie"));
                        s.finished_at = Some(now());
                    }
                }
            }
            if !ajout {
                break;
            }
        }
    }

    /// Recalcule le statut global depuis celui des étapes.
    pub fn refresh_status(&mut self) {
        self.status = if self.steps.iter().all(|s| s.status == StepStatus::Done) {
            PlanStatus::Done
        } else if self.est_termine() {
            PlanStatus::Incomplete
        } else {
            PlanStatus::Running
        };
        self.updated_at = now();
    }

    /// Avancement : étapes closes sur total.
    pub fn progress(&self) -> (usize, usize) {
        (
            self.steps.iter().filter(|s| s.est_close()).count(),
            self.steps.len(),
        )
    }

    /// Remet en attente tout ce qui n'a pas abouti, pour relancer un plan
    /// interrompu **sans refaire ce qui a réussi**.
    ///
    /// Les étapes `Done` gardent leur résultat ; les autres — y compris celles
    /// laissées en `Running` par une fermeture brutale, et celles écartées
    /// parce qu'une dépendance avait échoué — repartent de zéro.
    pub fn reset_unfinished(&mut self) {
        for s in &mut self.steps {
            if s.status != StepStatus::Done {
                s.status = StepStatus::Pending;
                s.error = None;
                s.result = None;
                s.started_at = None;
                s.finished_at = None;
            }
        }
        self.updated_at = now();
    }

    /// Contexte confié à une étape : l'objectif, puis le résultat de ses
    /// dépendances — et rien d'autre.
    pub fn context_for(&self, step: &PlanStep) -> String {
        let mut out = format!(
            "## Objectif du plan\n\n{}\n\n## Ton étape — {}\n\n{}\n",
            self.objective.trim(),
            step.title.trim(),
            step.instruction.trim()
        );
        let deps: Vec<&PlanStep> = step
            .depends_on
            .iter()
            .filter_map(|d| self.step(d))
            .collect();
        if !deps.is_empty() {
            out.push_str("\n## Résultats dont tu dépends\n");
            for d in deps {
                let contenu = d
                    .result
                    .as_deref()
                    .map(str::trim)
                    .filter(|r| !r.is_empty())
                    .unwrap_or("(aucun résultat)");
                out.push_str(&format!("\n### {} — {}\n\n{contenu}\n", d.id, d.title));
            }
        }
        out.push_str(
            "\nRéponds uniquement sur ton étape. Ton résultat sera transmis tel quel \
             aux étapes suivantes.\n",
        );
        out
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ===== Ébauche produite par un modèle =====

/// Extrait le premier objet JSON d'une réponse de modèle.
///
/// Un modèle encadre volontiers son JSON de prose ou d'une clôture ```json.
/// Exiger un JSON pur ferait échouer la décomposition une fois sur deux ; on
/// équilibre donc les accolades pour retrouver l'objet, en ignorant celles
/// qui apparaissent à l'intérieur d'une chaîne.
pub fn extraire_json(texte: &str) -> Option<&str> {
    let debut = texte.find('{')?;
    let octets = texte.as_bytes();
    let mut profondeur = 0i32;
    let mut dans_chaine = false;
    let mut echappe = false;
    for (i, &c) in octets.iter().enumerate().skip(debut) {
        if dans_chaine {
            if echappe {
                echappe = false;
            } else if c == b'\\' {
                echappe = true;
            } else if c == b'"' {
                dans_chaine = false;
            }
            continue;
        }
        match c {
            b'"' => dans_chaine = true,
            b'{' => profondeur += 1,
            b'}' => {
                profondeur -= 1;
                if profondeur == 0 {
                    return texte.get(debut..=i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Étape telle que la propose un modèle : ni agent ni modèle, que la structure.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EbaucheEtape {
    pub id: String,
    pub title: String,
    pub instruction: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Agent suggéré, s'il en propose un.
    #[serde(default)]
    pub agent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ebauche {
    #[serde(default)]
    pub title: String,
    pub steps: Vec<EbaucheEtape>,
}

/// Consigne de décomposition envoyée au modèle.
pub fn prompt_decomposition(objectif: &str, agents: &[String]) -> String {
    format!(
        "Découpe l'objectif suivant en étapes exécutables.\n\n\
         Objectif : {objectif}\n\n\
         Agents disponibles : {}\n\n\
         Réponds UNIQUEMENT par un objet JSON de la forme :\n\
         {{\"title\": \"…\", \"steps\": [{{\"id\": \"s1\", \"title\": \"…\", \
         \"instruction\": \"…\", \"dependsOn\": [], \"agent\": \"…\"}}]}}\n\n\
         Règles : identifiants courts et uniques ; `dependsOn` ne cite que des \
         identifiants déjà définis ; aucune dépendance circulaire ; une étape \
         par unité de travail réellement séparable ; au plus {MAX_ETAPES} étapes.",
        agents.join(", ")
    )
}

/// Convertit une ébauche en plan exécutable.
///
/// L'agent proposé n'est retenu que s'il existe vraiment dans le coffre :
/// un modèle invente volontiers un nom plausible, et une étape confiée à un
/// agent inexistant tournerait sans prompt système.
pub fn plan_depuis_ebauche(
    ebauche: Ebauche,
    objectif: &str,
    agents_connus: &[String],
    agent_defaut: &str,
    provider: Option<String>,
    model: &str,
) -> Result<Plan, String> {
    if ebauche.steps.is_empty() {
        return Err("le modèle n'a proposé aucune étape".into());
    }
    let steps: Vec<PlanStep> = ebauche
        .steps
        .into_iter()
        .map(|e| {
            let agent = e
                .agent
                .filter(|a| agents_connus.iter().any(|c| c == a))
                .unwrap_or_else(|| agent_defaut.to_string());
            let mut s = PlanStep::new(e.id, e.title, e.instruction, agent, model);
            s.provider = provider.clone();
            s.depends_on = e.depends_on;
            s
        })
        .collect();
    let titre = if ebauche.title.trim().is_empty() {
        objectif.chars().take(60).collect()
    } else {
        ebauche.title
    };
    let plan = Plan::new(titre, objectif.to_string(), steps);
    plan.validate()?;
    Ok(plan)
}

// ===== Stockage =====

pub struct PlanStore {
    inner: JsonStore,
}

impl PlanStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            inner: JsonStore::new(dir),
        }
    }

    pub fn save(&self, plan: &Plan) -> Result<(), String> {
        plan.validate()?;
        self.inner.save(&plan.id, plan)
    }

    pub fn load(&self, id: &str) -> Result<Plan, String> {
        self.inner.load(id)
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        self.inner.delete(id)
    }

    /// Plans enregistrés, le plus récemment modifié en tête.
    pub fn list(&self) -> Vec<Plan> {
        let mut plans: Vec<Plan> = self.inner.load_all();
        plans.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
        plans
    }
}

/// Stockage des plans + annulations en cours.
pub struct PlanManager {
    store: PlanStore,
    /// Sérialise les cycles lecture-modification-écriture d'un même plan.
    ecriture: tokio::sync::Mutex<()>,
    annulations:
        tokio::sync::RwLock<HashMap<String, std::sync::Arc<std::sync::atomic::AtomicBool>>>,
}

impl PlanManager {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            store: PlanStore::new(dir),
            ecriture: tokio::sync::Mutex::new(()),
            annulations: tokio::sync::RwLock::new(HashMap::new()),
        }
    }

    pub fn list(&self) -> Vec<Plan> {
        self.store.list()
    }

    pub fn load(&self, id: &str) -> Result<Plan, String> {
        self.store.load(id)
    }

    pub async fn save(&self, plan: &Plan) -> Result<(), String> {
        let _guard = self.ecriture.lock().await;
        self.store.save(plan)
    }

    pub async fn delete(&self, id: &str) -> Result<(), String> {
        self.cancel(id).await;
        let _guard = self.ecriture.lock().await;
        self.annulations.write().await.remove(id);
        self.store.delete(id)
    }

    /// Arme un drapeau d'annulation. Remplace le précédent : une relance ne
    /// doit pas hériter d'une annulation demandée pour l'exécution d'avant.
    pub async fn begin(&self, id: &str) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.annulations
            .write()
            .await
            .insert(id.to_string(), flag.clone());
        flag
    }

    pub async fn cancel(&self, id: &str) -> bool {
        match self.annulations.read().await.get(id) {
            Some(flag) => {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    pub async fn finish(&self, id: &str) {
        self.annulations.write().await.remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn etape(id: &str) -> PlanStep {
        PlanStep::new(
            id,
            format!("Étape {id}"),
            "fais quelque chose",
            "assistant",
            "qwen3:8b",
        )
    }

    fn plan(steps: Vec<PlanStep>) -> Plan {
        Plan::new("Livraison".into(), "livrer la fonctionnalité".into(), steps)
    }

    // ===== Validation =====

    #[test]
    fn un_plan_bien_forme_est_accepte() {
        let p = plan(vec![etape("a"), etape("b").depending_on(&["a"])]);
        assert!(p.validate().is_ok());
    }

    #[test]
    fn un_plan_vide_est_refuse() {
        assert!(plan(vec![]).validate().is_err());
    }

    #[test]
    fn un_identifiant_en_double_est_refuse() {
        let p = plan(vec![etape("a"), etape("a")]);
        assert!(p.validate().unwrap_err().contains("deux fois"));
    }

    #[test]
    fn une_dependance_inexistante_est_refusee() {
        let p = plan(vec![etape("a").depending_on(&["fantome"])]);
        assert!(p.validate().unwrap_err().contains("n'existe pas"));
    }

    #[test]
    fn une_etape_ne_peut_pas_dependre_d_elle_meme() {
        let p = plan(vec![etape("a").depending_on(&["a"])]);
        assert!(p.validate().unwrap_err().contains("elle-même"));
    }

    #[test]
    fn un_cycle_est_detecte() {
        // À l'exécution, un cycle ne lèverait pas d'erreur : aucune étape ne
        // deviendrait prête et le plan resterait « en cours » sans rien faire.
        let p = plan(vec![
            etape("a").depending_on(&["c"]),
            etape("b").depending_on(&["a"]),
            etape("c").depending_on(&["b"]),
        ]);
        let err = p.validate().unwrap_err();
        assert!(err.contains("circulaires"), "message : {err}");
        assert!(err.contains('a') && err.contains('b') && err.contains('c'));
    }

    #[test]
    fn un_cycle_partiel_n_empeche_pas_de_le_detecter() {
        // Une branche saine ne doit pas masquer le cycle voisin.
        let p = plan(vec![
            etape("libre"),
            etape("x").depending_on(&["y"]),
            etape("y").depending_on(&["x"]),
        ]);
        assert!(p.validate().is_err());
    }

    #[test]
    fn le_nombre_d_etapes_est_borne() {
        let trop: Vec<PlanStep> = (0..=MAX_ETAPES).map(|i| etape(&format!("s{i}"))).collect();
        assert!(plan(trop).validate().is_err());
    }

    // ===== Étapes prêtes =====

    #[test]
    fn les_etapes_sans_dependance_partent_ensemble() {
        // C'est l'intérêt du plan : deux branches indépendantes ne s'attendent pas.
        let p = plan(vec![
            etape("a"),
            etape("b"),
            etape("c").depending_on(&["a"]),
        ]);
        let pretes: Vec<&str> = p.ready_steps().iter().map(|s| s.id.as_str()).collect();
        assert_eq!(pretes, ["a", "b"]);
    }

    #[test]
    fn une_etape_attend_toutes_ses_dependances() {
        let mut p = plan(vec![
            etape("a"),
            etape("b"),
            etape("c").depending_on(&["a", "b"]),
        ]);
        p.mark_done("a", "fait".into());
        assert!(!p.ready_steps().iter().any(|s| s.id == "c"));
        p.mark_done("b", "fait".into());
        assert!(p.ready_steps().iter().any(|s| s.id == "c"));
    }

    #[test]
    fn une_etape_en_cours_n_est_plus_prete() {
        // Sinon une relance la lancerait une seconde fois en parallèle.
        let mut p = plan(vec![etape("a")]);
        p.mark_running("a");
        assert!(p.ready_steps().is_empty());
    }

    // ===== Échec et propagation =====

    #[test]
    fn un_echec_ecarte_toute_la_chaine_dependante() {
        let mut p = plan(vec![
            etape("a"),
            etape("b").depending_on(&["a"]),
            etape("c").depending_on(&["b"]),
        ]);
        p.mark_failed("a", "modèle injoignable".into());
        assert_eq!(p.step("b").unwrap().status, StepStatus::Skipped);
        assert_eq!(p.step("c").unwrap().status, StepStatus::Skipped);
        assert!(p.step("c").unwrap().error.as_ref().unwrap().contains('a'));
    }

    #[test]
    fn un_echec_laisse_vivre_les_branches_independantes() {
        let mut p = plan(vec![
            etape("a"),
            etape("libre"),
            etape("b").depending_on(&["a"]),
        ]);
        p.mark_failed("a", "boum".into());
        assert_eq!(p.step("libre").unwrap().status, StepStatus::Pending);
        assert!(p.ready_steps().iter().any(|s| s.id == "libre"));
    }

    #[test]
    fn un_echec_ne_defait_pas_une_etape_deja_reussie() {
        let mut p = plan(vec![etape("a"), etape("b").depending_on(&["a"])]);
        p.mark_done("b", "déjà fait".into());
        p.mark_failed("a", "boum".into());
        assert_eq!(p.step("b").unwrap().status, StepStatus::Done);
    }

    // ===== Statut global =====

    #[test]
    fn un_plan_tout_reussi_est_termine() {
        let mut p = plan(vec![etape("a"), etape("b")]);
        p.mark_done("a", "x".into());
        p.mark_done("b", "y".into());
        p.refresh_status();
        assert_eq!(p.status, PlanStatus::Done);
        assert_eq!(p.progress(), (2, 2));
    }

    #[test]
    fn un_plan_avec_un_echec_est_incomplet() {
        let mut p = plan(vec![etape("a"), etape("b").depending_on(&["a"])]);
        p.mark_failed("a", "boum".into());
        p.refresh_status();
        assert_eq!(p.status, PlanStatus::Incomplete);
    }

    #[test]
    fn un_plan_en_cours_reste_en_cours() {
        let mut p = plan(vec![etape("a"), etape("b")]);
        p.mark_done("a", "x".into());
        p.refresh_status();
        assert_eq!(p.status, PlanStatus::Running);
    }

    // ===== Reprise =====

    #[test]
    fn la_reprise_conserve_ce_qui_a_reussi() {
        let mut p = plan(vec![
            etape("a"),
            etape("b").depending_on(&["a"]),
            etape("c").depending_on(&["b"]),
        ]);
        p.mark_done("a", "résultat de a".into());
        p.mark_failed("b", "coupure".into());
        p.reset_unfinished();

        assert_eq!(p.step("a").unwrap().status, StepStatus::Done);
        assert_eq!(
            p.step("a").unwrap().result.as_deref(),
            Some("résultat de a")
        );
        assert_eq!(p.step("b").unwrap().status, StepStatus::Pending);
        assert!(p.step("b").unwrap().error.is_none());
        assert_eq!(p.step("c").unwrap().status, StepStatus::Pending);
        // b redevient exécutable, a n'est pas refaite.
        let pretes: Vec<&str> = p.ready_steps().iter().map(|s| s.id.as_str()).collect();
        assert_eq!(pretes, ["b"]);
    }

    #[test]
    fn une_etape_laissee_en_cours_est_reprise() {
        // Cas d'une fermeture brutale pendant l'exécution.
        let mut p = plan(vec![etape("a")]);
        p.mark_running("a");
        p.reset_unfinished();
        assert_eq!(p.step("a").unwrap().status, StepStatus::Pending);
    }

    // ===== Contexte d'étape =====

    #[test]
    fn le_contexte_porte_l_objectif_et_les_dependances() {
        let mut p = plan(vec![etape("a"), etape("b").depending_on(&["a"])]);
        p.mark_done("a", "voici le brouillon".into());
        let ctx = p.context_for(p.step("b").unwrap());
        assert!(ctx.contains("livrer la fonctionnalité"));
        assert!(ctx.contains("voici le brouillon"));
        assert!(ctx.contains("Étape b"));
    }

    #[test]
    fn le_contexte_exclut_les_etapes_non_liees() {
        // Le prompt doit rester court, et deux branches ne pas se polluer.
        let mut p = plan(vec![
            etape("a"),
            etape("autre"),
            etape("b").depending_on(&["a"]),
        ]);
        p.mark_done("a", "utile".into());
        p.mark_done("autre", "SECRET_SANS_RAPPORT".into());
        let ctx = p.context_for(p.step("b").unwrap());
        assert!(ctx.contains("utile"));
        assert!(!ctx.contains("SECRET_SANS_RAPPORT"));
    }

    #[test]
    fn une_etape_sans_dependance_n_a_pas_de_section_de_resultats() {
        let p = plan(vec![etape("a")]);
        assert!(!p
            .context_for(p.step("a").unwrap())
            .contains("dont tu dépends"));
    }

    // ===== Extraction du JSON d'ébauche =====

    #[test]
    fn extrait_le_json_entoure_de_prose() {
        // Un modèle encadre volontiers son JSON d'explications.
        let t = "Bien sûr !\n```json\n{\"title\":\"x\",\"steps\":[]}\n```\nVoilà.";
        assert_eq!(extraire_json(t), Some("{\"title\":\"x\",\"steps\":[]}"));
    }

    #[test]
    fn extrait_un_json_a_objets_imbriques() {
        let t = "{\"a\":{\"b\":1},\"c\":2} et du bruit";
        assert_eq!(extraire_json(t), Some("{\"a\":{\"b\":1},\"c\":2}"));
    }

    #[test]
    fn une_accolade_dans_une_chaine_ne_trompe_pas_l_extraction() {
        let t = r#"{"instruction":"écris } puis {","x":1}"#;
        assert_eq!(extraire_json(t), Some(t));
    }

    #[test]
    fn une_accolade_echappee_ne_trompe_pas_non_plus() {
        let t = r#"{"s":"guillemet \" et }","y":2}"#;
        assert_eq!(extraire_json(t), Some(t));
    }

    #[test]
    fn un_json_tronque_ne_produit_rien() {
        assert_eq!(extraire_json("{\"a\": [1, 2"), None);
        assert_eq!(extraire_json("aucune accolade"), None);
    }

    // ===== Ébauche vers plan =====

    fn ebauche(json: &str) -> Ebauche {
        serde_json::from_str(extraire_json(json).unwrap()).unwrap()
    }

    #[test]
    fn une_ebauche_devient_un_plan_valide() {
        let e = ebauche(
            r#"{"title":"Refonte","steps":[
                {"id":"s1","title":"Analyser","instruction":"lis le code","dependsOn":[]},
                {"id":"s2","title":"Écrire","instruction":"rédige","dependsOn":["s1"]}]}"#,
        );
        let p = plan_depuis_ebauche(
            e,
            "refondre",
            &["assistant".into()],
            "assistant",
            None,
            "qwen3:8b",
        )
        .unwrap();
        assert_eq!(p.title, "Refonte");
        assert_eq!(p.steps.len(), 2);
        assert_eq!(p.steps[1].depends_on, ["s1"]);
        assert_eq!(p.steps[0].model, "qwen3:8b");
    }

    #[test]
    fn un_agent_invente_retombe_sur_l_agent_par_defaut() {
        // Un modèle propose volontiers un nom plausible mais inexistant ;
        // l'étape tournerait alors sans prompt système.
        let e = ebauche(
            r#"{"steps":[{"id":"s1","title":"T","instruction":"I","agent":"architecte-fantôme"}]}"#,
        );
        let p =
            plan_depuis_ebauche(e, "obj", &["assistant".into()], "assistant", None, "m").unwrap();
        assert_eq!(p.steps[0].agent, "assistant");
    }

    #[test]
    fn un_agent_reellement_present_est_conserve() {
        let e =
            ebauche(r#"{"steps":[{"id":"s1","title":"T","instruction":"I","agent":"relecteur"}]}"#);
        let p = plan_depuis_ebauche(
            e,
            "obj",
            &["assistant".into(), "relecteur".into()],
            "assistant",
            None,
            "m",
        )
        .unwrap();
        assert_eq!(p.steps[0].agent, "relecteur");
    }

    #[test]
    fn une_ebauche_cyclique_est_rejetee() {
        let e = ebauche(
            r#"{"steps":[
                {"id":"a","title":"A","instruction":"i","dependsOn":["b"]},
                {"id":"b","title":"B","instruction":"i","dependsOn":["a"]}]}"#,
        );
        assert!(
            plan_depuis_ebauche(e, "obj", &["assistant".into()], "assistant", None, "m").is_err()
        );
    }

    #[test]
    fn une_ebauche_sans_etape_est_rejetee() {
        let e = ebauche(r#"{"steps":[]}"#);
        assert!(plan_depuis_ebauche(e, "obj", &[], "assistant", None, "m").is_err());
    }

    #[test]
    fn un_titre_absent_est_derive_de_l_objectif() {
        let e = ebauche(r#"{"steps":[{"id":"s1","title":"T","instruction":"I"}]}"#);
        let p = plan_depuis_ebauche(e, "mon objectif", &[], "assistant", None, "m").unwrap();
        assert_eq!(p.title, "mon objectif");
    }

    #[test]
    fn le_prompt_de_decomposition_liste_les_agents() {
        let p = prompt_decomposition("livrer", &["assistant".into(), "relecteur".into()]);
        assert!(p.contains("livrer"));
        assert!(p.contains("assistant, relecteur"));
        assert!(p.contains("dependsOn"));
    }

    // ===== Stockage =====

    #[test]
    fn aller_retour_et_tri_par_recence() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new(dir.path());
        let mut vieux = plan(vec![etape("a")]);
        vieux.updated_at = 1_000;
        let mut neuf = plan(vec![etape("a")]);
        neuf.updated_at = 2_000;
        store.save(&vieux).unwrap();
        store.save(&neuf).unwrap();
        let liste = store.list();
        assert_eq!(liste.len(), 2);
        assert_eq!(liste[0].id, neuf.id);
        assert_eq!(store.load(&vieux.id).unwrap().id, vieux.id);
    }

    #[test]
    fn un_plan_invalide_n_est_pas_enregistre() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new(dir.path());
        let p = plan(vec![etape("a").depending_on(&["a"])]);
        assert!(store.save(&p).is_err());
        assert!(store.list().is_empty());
    }
}
