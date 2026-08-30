//! Équipes d'agents et orchestration des tours de parole.
//!
//! Une équipe réunit plusieurs agents du coffre — chacun avec **son propre
//! fournisseur et son propre modèle**, ce qui permet de faire tourner un rôle
//! bavard sur un modèle local et un rôle décisif sur un modèle plus capable.
//!
//! L'équipe est une notion du harness, pas du coffre : elle assemble des
//! agents que le coffre définit, sans rien y écrire. Le coffre reste utilisable
//! par n'importe quel autre harness, qui composera ses équipes à sa façon.
//!
//! Deux stratégies :
//!
//! - **Tour de rôle** — chacun parle à son tour, dans l'ordre déclaré. Simple,
//!   prévisible, et le coût est borné d'avance.
//! - **Superviseur** — le premier membre distribue la parole. Plus souple,
//!   mais il faut lire sa réponse pour savoir qui parle ensuite : la
//!   désignation est donc explicite (`SUIVANT: <agent>`) et, faute de
//!   désignation lisible, on retombe sur la rotation plutôt que de bloquer.
//!
//! Toute exécution est **bornée** par `max_turns` : une équipe qui s'auto-
//! alimente sans garde-fou consomme un budget sans fin.

use crate::store::JsonStore;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Marqueur par lequel un agent déclare le travail terminé.
pub const MARQUEUR_FIN: &str = "TERMINÉ";

/// Préfixe de désignation utilisé par le superviseur.
pub const PREFIXE_SUIVANT: &str = "SUIVANT:";

/// Nombre de tours au-delà duquel on refuse une équipe.
pub const MAX_TOURS_PLAFOND: u32 = 50;

// ===== Modèle =====

/// Un membre : quel agent, avec quel modèle, et pour quel rôle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamMember {
    /// Nom de l'agent du coffre (`IA/agents/<nom>.md`).
    pub agent: String,
    /// Fournisseur ciblé. `None` = repli automatique sur le pool.
    #[serde(default)]
    pub provider: Option<String>,
    pub model: String,
    /// Consigne propre à ce membre **dans cette équipe**, en plus du prompt
    /// de l'agent. Permet de réutiliser le même agent avec un rôle différent.
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TeamStrategy {
    /// Chacun parle à son tour, dans l'ordre déclaré.
    RoundRobin,
    /// Le premier membre distribue la parole.
    Supervisor,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Team {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub members: Vec<TeamMember>,
    pub strategy: TeamStrategy,
    /// Garde-fou : nombre maximal de tours de parole par exécution.
    pub max_turns: u32,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Team {
    pub fn new(
        name: String,
        description: String,
        members: Vec<TeamMember>,
        strategy: TeamStrategy,
        max_turns: u32,
    ) -> Self {
        let at = now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            description,
            members,
            strategy,
            max_turns,
            created_at: at,
            updated_at: at,
        }
    }

    /// Vérifie qu'une équipe est exécutable.
    ///
    /// Ces contrôles sont faits une fois à l'enregistrement, pour qu'une
    /// exécution ne puisse pas échouer à mi-parcours sur une équipe mal formée
    /// — le budget déjà consommé serait perdu.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("le nom de l'équipe est requis".into());
        }
        if self.members.is_empty() {
            return Err("une équipe compte au moins un membre".into());
        }
        if self.strategy == TeamStrategy::Supervisor && self.members.len() < 2 {
            return Err("la stratégie superviseur demande au moins deux membres : \
                 un superviseur et un exécutant"
                .into());
        }
        if self.max_turns == 0 || self.max_turns > MAX_TOURS_PLAFOND {
            return Err(format!(
                "max_turns doit être compris entre 1 et {MAX_TOURS_PLAFOND}"
            ));
        }
        for m in &self.members {
            if m.agent.trim().is_empty() {
                return Err("chaque membre désigne un agent".into());
            }
            if m.model.trim().is_empty() {
                return Err(format!("le membre « {} » n'a pas de modèle", m.agent));
            }
        }
        // Deux fois le même agent rendrait toute désignation ambiguë et le
        // fil de conversation illisible.
        let mut vus = std::collections::HashSet::new();
        for m in &self.members {
            if !vus.insert(m.agent.as_str()) {
                return Err(format!("l'agent « {} » est présent deux fois", m.agent));
            }
        }
        Ok(())
    }

    /// Le superviseur, en stratégie superviseur : le premier membre.
    pub fn supervisor(&self) -> Option<&TeamMember> {
        match self.strategy {
            TeamStrategy::Supervisor => self.members.first(),
            TeamStrategy::RoundRobin => None,
        }
    }

    pub fn member(&self, agent: &str) -> Option<&TeamMember> {
        self.members.iter().find(|m| m.agent == agent)
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ===== Tours de parole =====

/// Qui parle au tour `tour` (numéroté à partir de 0).
///
/// `designe` porte l'agent désigné par le dernier passage du superviseur.
/// Renvoie `None` quand le budget de tours est épuisé.
pub fn prochain_intervenant(team: &Team, tour: u32, designe: Option<&str>) -> Option<TeamMember> {
    if tour >= team.max_turns || team.members.is_empty() {
        return None;
    }
    match team.strategy {
        TeamStrategy::RoundRobin => {
            let i = (tour as usize) % team.members.len();
            Some(team.members[i].clone())
        }
        TeamStrategy::Supervisor => {
            // Le superviseur parle un tour sur deux : il ouvre, puis reprend
            // la main après chaque exécutant.
            if tour.is_multiple_of(2) {
                return team.members.first().cloned();
            }
            if let Some(nom) = designe {
                if let Some(m) = team.member(nom) {
                    // Le superviseur ne peut pas se désigner lui-même : ce
                    // serait un tour de parole perdu, et sans fin.
                    if Some(m.agent.as_str()) != team.members.first().map(|s| s.agent.as_str()) {
                        return Some(m.clone());
                    }
                }
            }
            // Pas de désignation lisible : rotation sur les exécutants, pour
            // avancer quand même plutôt que de s'arrêter sur un malentendu.
            let executants = team.members.len() - 1;
            if executants == 0 {
                return None;
            }
            let i = ((tour as usize / 2) % executants) + 1;
            Some(team.members[i].clone())
        }
    }
}

/// Extrait l'agent désigné dans la réponse du superviseur.
///
/// La ligne explicite `SUIVANT: <agent>` prime. À défaut, on cherche le nom
/// d'un membre cité dans le texte — un modèle local suit rarement un protocole
/// à la lettre, et refuser d'interpréter bloquerait l'équipe.
pub fn designation(team: &Team, reponse: &str) -> Option<String> {
    for ligne in reponse.lines() {
        let ligne = ligne.trim();
        let sans_puce = ligne.trim_start_matches(['-', '*', '#', ' ']);
        if let Some(reste) = sans_puce
            .strip_prefix(PREFIXE_SUIVANT)
            .or_else(|| sans_puce.strip_prefix("SUIVANT :"))
        {
            let nom = reste
                .trim()
                .trim_matches(['`', '*', '"', '.', '«', '»', ' ']);
            if let Some(m) = team.member(nom) {
                return Some(m.agent.clone());
            }
        }
    }
    // Repli : premier nom de membre cité comme mot entier.
    let mut trouve: Option<(usize, String)> = None;
    for m in team.members.iter().skip(1) {
        if let Some(pos) = position_mot_entier(reponse, &m.agent) {
            if trouve.as_ref().is_none_or(|(p, _)| pos < *p) {
                trouve = Some((pos, m.agent.clone()));
            }
        }
    }
    trouve.map(|(_, nom)| nom)
}

/// Position d'un nom cité comme mot entier (bornes non alphanumériques).
fn position_mot_entier(texte: &str, mot: &str) -> Option<usize> {
    if mot.is_empty() {
        return None;
    }
    let mut depart = 0usize;
    while let Some(rel) = texte[depart..].find(mot) {
        let debut = depart + rel;
        let fin = debut + mot.len();
        let avant_ok = texte[..debut]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric() && c != '-' && c != '_');
        let apres_ok = texte[fin..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric() && c != '-' && c != '_');
        if avant_ok && apres_ok {
            return Some(debut);
        }
        depart = fin;
    }
    None
}

/// Vrai si la réponse déclare le travail terminé.
///
/// Le marqueur doit tenir **sa propre ligne** : accepter n'importe quelle
/// occurrence arrêterait l'équipe dès qu'un agent écrit « le travail sera
/// TERMINÉ quand… ».
pub fn est_termine(reponse: &str) -> bool {
    reponse.lines().any(|l| {
        let nu = l.trim().trim_matches(['*', '#', '`', '.', '!', ' ']);
        // `eq_ignore_ascii_case` ne replie pas le « é » : « terminé » ne
        // correspondrait pas à « TERMINÉ ». On passe par une mise en
        // majuscules Unicode. La variante sans accent est acceptée, les
        // modèles la produisent souvent.
        let haut = nu.to_uppercase();
        haut == MARQUEUR_FIN || haut == "TERMINE"
    })
}

/// Consigne d'équipe ajoutée au prompt système de l'agent.
///
/// Elle complète le prompt de l'agent — elle ne le remplace pas : l'agent
/// reste lui-même, on lui explique seulement le cadre collectif.
pub fn briefing(team: &Team, membre: &TeamMember, objectif: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "## Travail en équipe — {}\n\n{}\n\n",
        team.name,
        team.description.trim()
    ));
    out.push_str(&format!("**Objectif commun** : {}\n\n", objectif.trim()));
    out.push_str(&format!("Tu interviens en tant que **{}**.", membre.agent));
    if let Some(role) = membre
        .role
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
    {
        out.push_str(&format!(" Ton rôle ici : {role}"));
    }
    out.push_str("\n\n**Composition de l'équipe**\n");
    for m in &team.members {
        let r = m.role.as_deref().unwrap_or("—");
        out.push_str(&format!("- `{}` : {r}\n", m.agent));
    }

    out.push_str("\n**Règles du tour de parole**\n");
    out.push_str(
        "- Les messages des autres membres sont attribués par leur nom : lis-les avant \
         de répondre et ne répète pas ce qui a déjà été dit.\n",
    );
    match team.strategy {
        TeamStrategy::Supervisor => {
            let superviseur = team
                .members
                .first()
                .map(|m| m.agent.as_str())
                .unwrap_or("le superviseur");
            if membre.agent == superviseur {
                out.push_str(&format!(
                    "- Tu distribues la parole. Termine ton message par une ligne \
                     `{PREFIXE_SUIVANT} <agent>` désignant le prochain intervenant.\n"
                ));
            } else {
                out.push_str(&format!(
                    "- `{superviseur}` distribue la parole. Réponds sur ta part du \
                     travail, puis rends la main.\n"
                ));
            }
        }
        TeamStrategy::RoundRobin => {
            out.push_str("- Chacun parle à son tour, dans l'ordre ci-dessus.\n");
        }
    }
    out.push_str(&format!(
        "- Quand l'objectif est atteint, écris `{MARQUEUR_FIN}` seul sur une ligne.\n"
    ));
    out
}

// ===== Stockage =====

pub struct TeamStore {
    inner: JsonStore,
}

impl TeamStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            inner: JsonStore::new(dir),
        }
    }

    pub fn save(&self, team: &Team) -> Result<(), String> {
        team.validate()?;
        self.inner.save(&team.id, team)
    }

    pub fn load(&self, id: &str) -> Result<Team, String> {
        self.inner.load(id)
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        self.inner.delete(id)
    }

    /// Équipes enregistrées, triées par nom pour un affichage stable.
    pub fn list(&self) -> Vec<Team> {
        let mut teams: Vec<Team> = self.inner.load_all();
        teams.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        teams
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn membre(agent: &str) -> TeamMember {
        TeamMember {
            agent: agent.to_string(),
            provider: None,
            model: "qwen3:8b".to_string(),
            role: None,
        }
    }

    fn equipe(strategy: TeamStrategy, agents: &[&str]) -> Team {
        Team::new(
            "Revue".into(),
            "Relit et corrige.".into(),
            agents.iter().map(|a| membre(a)).collect(),
            strategy,
            6,
        )
    }

    // ===== Validation =====

    #[test]
    fn une_equipe_bien_formee_est_acceptee() {
        assert!(equipe(TeamStrategy::RoundRobin, &["a", "b"])
            .validate()
            .is_ok());
    }

    #[test]
    fn une_equipe_sans_membre_est_refusee() {
        assert!(equipe(TeamStrategy::RoundRobin, &[]).validate().is_err());
    }

    #[test]
    fn le_superviseur_exige_un_executant() {
        // Un superviseur seul se passerait la parole à lui-même sans fin.
        let seul = equipe(TeamStrategy::Supervisor, &["chef"]);
        assert!(seul.validate().is_err());
        assert!(equipe(TeamStrategy::Supervisor, &["chef", "dev"])
            .validate()
            .is_ok());
    }

    #[test]
    fn un_agent_en_double_est_refuse() {
        // Une désignation serait ambiguë et le fil illisible.
        let t = equipe(TeamStrategy::RoundRobin, &["a", "a"]);
        assert!(t.validate().unwrap_err().contains("deux fois"));
    }

    #[test]
    fn le_budget_de_tours_est_borne() {
        let mut t = equipe(TeamStrategy::RoundRobin, &["a"]);
        t.max_turns = 0;
        assert!(t.validate().is_err());
        t.max_turns = MAX_TOURS_PLAFOND + 1;
        assert!(t.validate().is_err());
        t.max_turns = MAX_TOURS_PLAFOND;
        assert!(t.validate().is_ok());
    }

    #[test]
    fn un_membre_sans_modele_est_refuse() {
        let mut t = equipe(TeamStrategy::RoundRobin, &["a"]);
        t.members[0].model = "  ".into();
        assert!(t.validate().unwrap_err().contains("modèle"));
    }

    // ===== Tour de rôle =====

    #[test]
    fn le_tour_de_role_boucle_dans_l_ordre() {
        let t = equipe(TeamStrategy::RoundRobin, &["a", "b", "c"]);
        let noms: Vec<String> = (0..6)
            .filter_map(|i| prochain_intervenant(&t, i, None))
            .map(|m| m.agent)
            .collect();
        assert_eq!(noms, ["a", "b", "c", "a", "b", "c"]);
    }

    #[test]
    fn le_budget_epuise_arrete_l_equipe() {
        let mut t = equipe(TeamStrategy::RoundRobin, &["a", "b"]);
        t.max_turns = 3;
        assert!(prochain_intervenant(&t, 2, None).is_some());
        assert!(prochain_intervenant(&t, 3, None).is_none());
    }

    // ===== Superviseur =====

    #[test]
    fn le_superviseur_reprend_la_main_un_tour_sur_deux() {
        let t = equipe(TeamStrategy::Supervisor, &["chef", "dev", "test"]);
        assert_eq!(prochain_intervenant(&t, 0, None).unwrap().agent, "chef");
        assert_eq!(
            prochain_intervenant(&t, 1, Some("test")).unwrap().agent,
            "test"
        );
        assert_eq!(prochain_intervenant(&t, 2, None).unwrap().agent, "chef");
    }

    #[test]
    fn sans_designation_lisible_on_tourne_sur_les_executants() {
        // Un modèle local suit mal un protocole : l'équipe doit avancer.
        let t = equipe(TeamStrategy::Supervisor, &["chef", "dev", "test"]);
        assert_eq!(prochain_intervenant(&t, 1, None).unwrap().agent, "dev");
        assert_eq!(prochain_intervenant(&t, 3, None).unwrap().agent, "test");
        assert_eq!(prochain_intervenant(&t, 5, None).unwrap().agent, "dev");
    }

    #[test]
    fn le_superviseur_ne_peut_pas_se_designer_lui_meme() {
        // Sinon il monopolise la parole jusqu'à épuisement du budget.
        let t = equipe(TeamStrategy::Supervisor, &["chef", "dev"]);
        assert_eq!(
            prochain_intervenant(&t, 1, Some("chef")).unwrap().agent,
            "dev"
        );
    }

    #[test]
    fn une_designation_inconnue_retombe_sur_la_rotation() {
        let t = equipe(TeamStrategy::Supervisor, &["chef", "dev"]);
        assert_eq!(
            prochain_intervenant(&t, 1, Some("fantôme")).unwrap().agent,
            "dev"
        );
    }

    // ===== Désignation =====

    #[test]
    fn la_ligne_explicite_prime() {
        let t = equipe(TeamStrategy::Supervisor, &["chef", "dev", "test"]);
        let reponse = "Il faut d'abord relire le code de dev.\nSUIVANT: test";
        assert_eq!(designation(&t, reponse).as_deref(), Some("test"));
    }

    #[test]
    fn la_designation_tolere_puces_et_ponctuation() {
        let t = equipe(TeamStrategy::Supervisor, &["chef", "dev", "test"]);
        for forme in [
            "- SUIVANT: `test`",
            "**SUIVANT:** test",
            "SUIVANT : test.",
            "SUIVANT:    test   ",
        ] {
            assert_eq!(
                designation(&t, forme).as_deref(),
                Some("test"),
                "forme non reconnue : {forme}"
            );
        }
    }

    #[test]
    fn a_defaut_le_premier_membre_cite_est_retenu() {
        let t = equipe(TeamStrategy::Supervisor, &["chef", "dev", "test"]);
        assert_eq!(
            designation(&t, "Je passe la main à dev, puis test verra.").as_deref(),
            Some("dev")
        );
    }

    #[test]
    fn le_superviseur_n_est_jamais_retenu_par_le_repli() {
        let t = equipe(TeamStrategy::Supervisor, &["chef", "dev"]);
        assert_eq!(designation(&t, "chef reprend la main").as_deref(), None);
    }

    #[test]
    fn un_nom_inclus_dans_un_mot_ne_compte_pas() {
        // « dev » ne doit pas être trouvé dans « développement ».
        let t = equipe(TeamStrategy::Supervisor, &["chef", "dev"]);
        assert_eq!(designation(&t, "le développement avance").as_deref(), None);
        assert_eq!(designation(&t, "à toi, dev !").as_deref(), Some("dev"));
    }

    #[test]
    fn sans_designation_ni_citation_on_ne_devine_pas() {
        let t = equipe(TeamStrategy::Supervisor, &["chef", "dev"]);
        assert_eq!(designation(&t, "Continuons.").as_deref(), None);
    }

    // ===== Fin de travail =====

    #[test]
    fn le_marqueur_seul_sur_sa_ligne_arrete_l_equipe() {
        assert!(est_termine("Tout est relu.\nTERMINÉ"));
        assert!(est_termine("**TERMINÉ**"));
        assert!(est_termine("terminé"));
        // Variante sans accent : les modèles la produisent souvent.
        assert!(est_termine("TERMINE"));
    }

    #[test]
    fn le_marqueur_cite_dans_une_phrase_n_arrete_rien() {
        // Sinon l'équipe s'arrêterait dès qu'un agent parle de finir.
        assert!(!est_termine(
            "le travail sera TERMINÉ quand les tests passeront"
        ));
        assert!(!est_termine("j'ai terminé ma part, à toi"));
    }

    // ===== Briefing =====

    #[test]
    fn le_briefing_enonce_le_protocole_du_superviseur() {
        let t = equipe(TeamStrategy::Supervisor, &["chef", "dev"]);
        let chef = briefing(&t, &t.members[0], "livrer la fonctionnalité");
        assert!(chef.contains("Tu distribues la parole"));
        assert!(chef.contains(PREFIXE_SUIVANT));
        let dev = briefing(&t, &t.members[1], "livrer la fonctionnalité");
        assert!(dev.contains("distribue la parole"));
        assert!(!dev.contains("Tu distribues la parole"));
    }

    #[test]
    fn le_briefing_liste_l_equipe_et_l_objectif() {
        let t = equipe(TeamStrategy::RoundRobin, &["a", "b"]);
        let b = briefing(&t, &t.members[0], "corriger le bug 42");
        assert!(b.contains("corriger le bug 42"));
        assert!(b.contains("`a`") && b.contains("`b`"));
        assert!(b.contains(MARQUEUR_FIN));
    }

    #[test]
    fn le_role_du_membre_apparait_dans_son_briefing() {
        let mut t = equipe(TeamStrategy::RoundRobin, &["a", "b"]);
        t.members[0].role = Some("relecteur".into());
        assert!(briefing(&t, &t.members[0], "obj").contains("relecteur"));
    }

    // ===== Stockage =====

    #[test]
    fn aller_retour_et_tri_par_nom() {
        let dir = tempfile::tempdir().unwrap();
        let store = TeamStore::new(dir.path());
        let mut zebre = equipe(TeamStrategy::RoundRobin, &["a"]);
        zebre.name = "Zèbre".into();
        let mut alpha = equipe(TeamStrategy::RoundRobin, &["b"]);
        alpha.name = "Alpha".into();
        store.save(&zebre).unwrap();
        store.save(&alpha).unwrap();
        let liste = store.list();
        assert_eq!(liste.len(), 2);
        assert_eq!(liste[0].name, "Alpha");
        assert_eq!(store.load(&zebre.id).unwrap(), zebre);
    }

    #[test]
    fn une_equipe_invalide_n_est_pas_enregistree() {
        // La validation à l'écriture évite qu'une exécution échoue à
        // mi-parcours, budget déjà consommé.
        let dir = tempfile::tempdir().unwrap();
        let store = TeamStore::new(dir.path());
        let t = equipe(TeamStrategy::Supervisor, &["seul"]);
        assert!(store.save(&t).is_err());
        assert!(store.list().is_empty());
    }
}
