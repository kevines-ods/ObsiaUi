//! Graphe du coffre : notes, liens et étiquettes.
//!
//! Obsidian n'expose aucune API externe officielle. Plutôt que de dépendre
//! d'un plugin communautaire — qui exigerait qu'Obsidian tourne, avec son
//! certificat auto-signé, et ne fournirait de toute façon pas le graphe déjà
//! dessiné — on reconstruit le graphe depuis les fichiers. Le coffre est du
//! Markdown : c'est exactement ce que fait Obsidian lui-même, et cela
//! fonctionne même s'il n'est pas installé.
//!
//! # Ce qui est reconnu
//!
//! - les liens internes `[[note]]`, avec alias `[[note|Affichage]]`, ancre
//!   `[[note#Section]]` et chemin `[[dossier/note]]` ;
//! - les inclusions `![[note]]`, qui sont des liens ;
//! - les liens Markdown `[texte](chemin.md)` — les URL externes sont ignorées ;
//! - les étiquettes `#étiquette`.
//!
//! # Ce qui est délibérément ignoré
//!
//! Le contenu des blocs de code et du code en ligne. Un `[[exemple]]` cité
//! dans un bloc de code n'est pas un lien : le compter créerait des arêtes
//! fantômes, et c'est précisément ce qu'on écrit dans la documentation d'un
//! coffre.
//!
//! # Résolution
//!
//! Obsidian résout `[[nom]]` par nom de fichier à l'échelle du coffre entier,
//! ce que le contrat du coffre assume explicitement (§6 : les noms de notes
//! doivent être uniques). On indexe donc par nom de base, avec repli sur le
//! chemin complet.

use crate::vault::VaultState;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Une note du graphe.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    /// Chemin relatif au coffre — identifiant du nœud.
    pub id: String,
    /// Nom affiché (nom de fichier sans extension).
    pub name: String,
    /// Dossier de premier niveau, pour colorer par zone du coffre.
    pub folder: String,
    pub tags: Vec<String>,
    pub out_degree: usize,
    pub in_degree: usize,
}

/// Un lien résolu entre deux notes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
}

/// Un lien qui ne pointe vers rien.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokenLink {
    pub from: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// Liens vers des notes inexistantes. Obsidian les montre aussi : ce sont
    /// souvent des fautes de frappe ou des notes à créer.
    pub broken: Vec<BrokenLink>,
    pub tags: Vec<String>,
}

/// Retire le contenu qui ne doit pas être analysé : blocs de code délimités
/// par des accents graves triples, et code en ligne.
///
/// Le texte retiré est remplacé par des espaces plutôt que supprimé, pour ne
/// pas recoller accidentellement deux fragments en un faux lien.
pub fn sans_code(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut reste = markdown;

    // Blocs délimités d'abord : ils peuvent contenir des accents simples.
    while let Some(debut) = reste.find("```") {
        out.push_str(&reste[..debut]);
        let apres = &reste[debut + 3..];
        match apres.find("```") {
            Some(fin) => {
                blanchir(&apres[..fin + 3], &mut out);
                reste = &apres[fin + 3..];
            }
            None => {
                // Bloc non fermé : tout le reste est du code.
                blanchir(apres, &mut out);
                reste = "";
            }
        }
    }
    out.push_str(reste);

    // Puis le code en ligne.
    let mut final_ = String::with_capacity(out.len());
    let mut reste = out.as_str();
    while let Some(debut) = reste.find('`') {
        final_.push_str(&reste[..debut]);
        let apres = &reste[debut + 1..];
        match apres.find('`') {
            Some(fin) => {
                blanchir(&apres[..fin + 1], &mut final_);
                reste = &apres[fin + 1..];
            }
            None => {
                final_.push_str(apres);
                reste = "";
            }
        }
    }
    final_.push_str(reste);
    final_
}

/// Remplace un fragment par des espaces, sauts de ligne conservés.
fn blanchir(fragment: &str, out: &mut String) {
    for c in fragment.chars() {
        out.push(if c == '\n' { '\n' } else { ' ' });
    }
}

/// Nettoie une cible de lien : retire l'alias et l'ancre.
fn cible_propre(brut: &str) -> Option<String> {
    let sans_alias = brut.split('|').next().unwrap_or(brut);
    let sans_ancre = sans_alias.split('#').next().unwrap_or(sans_alias);
    let nu = sans_ancre.trim();
    if nu.is_empty() {
        return None;
    }
    Some(nu.to_string())
}

/// Cibles des liens internes d'une note, dans l'ordre d'apparition.
pub fn extraire_liens(markdown: &str) -> Vec<String> {
    let texte = sans_code(markdown);
    let mut out = Vec::new();

    // Liens et inclusions en double crochets.
    let mut reste = texte.as_str();
    while let Some(debut) = reste.find("[[") {
        let apres = &reste[debut + 2..];
        match apres.find("]]") {
            Some(fin) => {
                if let Some(cible) = cible_propre(&apres[..fin]) {
                    out.push(cible);
                }
                reste = &apres[fin + 2..];
            }
            None => break,
        }
    }

    // Liens Markdown vers des fichiers du coffre.
    let mut reste = texte.as_str();
    while let Some(debut) = reste.find("](") {
        let apres = &reste[debut + 2..];
        match apres.find(')') {
            Some(fin) => {
                let cible = apres[..fin].trim();
                // Une URL externe n'est pas une arête du coffre.
                let externe = cible.starts_with("http://")
                    || cible.starts_with("https://")
                    || cible.starts_with("obsidian://")
                    || cible.starts_with("mailto:");
                if !externe {
                    if let Some(c) = cible_propre(cible) {
                        out.push(c);
                    }
                }
                reste = &apres[fin + 1..];
            }
            None => break,
        }
    }
    out
}

/// Étiquettes `#nom` d'une note.
///
/// Un titre Markdown (`# Titre`) n'en est pas une : le `#` y est suivi d'une
/// espace. Une étiquette purement numérique non plus — `#1` désigne une issue,
/// pas une catégorie.
pub fn extraire_tags(markdown: &str) -> Vec<String> {
    let texte = sans_code(markdown);
    let octets: Vec<char> = texte.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;

    while i < octets.len() {
        if octets[i] != '#' {
            i += 1;
            continue;
        }
        // Doit être en début de ligne ou précédé d'une espace : `abc#def`
        // n'est pas une étiquette, et `##` est un titre de niveau 2.
        let precede_ok = i == 0 || octets[i - 1].is_whitespace();
        i += 1;
        if !precede_ok {
            continue;
        }
        let debut = i;
        while i < octets.len()
            && (octets[i].is_alphanumeric() || matches!(octets[i], '-' | '_' | '/'))
        {
            i += 1;
        }
        let mot: String = octets[debut..i].iter().collect();
        if !mot.is_empty() && !mot.chars().all(|c| c.is_ascii_digit()) {
            out.push(mot);
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Nom de base d'un chemin, sans extension.
fn nom_de_base(chemin: &str) -> String {
    let fichier = chemin.rsplit('/').next().unwrap_or(chemin);
    fichier.trim_end_matches(".md").to_string()
}

/// Dossier de premier niveau, ou `.` à la racine.
fn dossier(chemin: &str) -> String {
    match chemin.split_once('/') {
        Some((tete, _)) => tete.to_string(),
        None => ".".to_string(),
    }
}

/// Résout une cible de lien vers un chemin du coffre.
///
/// Par chemin exact d'abord, puis par nom de base — c'est ainsi qu'Obsidian
/// procède, et le contrat du coffre garantit l'unicité des noms (§6).
pub fn resoudre(
    cible: &str,
    par_chemin: &BTreeSet<String>,
    par_nom: &HashMap<String, String>,
) -> Option<String> {
    let avec_ext = if cible.ends_with(".md") {
        cible.to_string()
    } else {
        format!("{cible}.md")
    };
    if par_chemin.contains(&avec_ext) {
        return Some(avec_ext);
    }
    if par_chemin.contains(cible) {
        return Some(cible.to_string());
    }
    par_nom.get(&nom_de_base(cible)).cloned()
}

impl VaultState {
    /// Construit le graphe du coffre.
    pub fn graph(&self) -> Result<VaultGraph, String> {
        self.ensure_configured()?;
        let notes = self.list_notes()?;

        // Index de résolution. Un nom présent deux fois ne peut pas être
        // départagé : on garde le premier et on laisse le chemin exact
        // trancher pour les autres.
        let par_chemin: BTreeSet<String> = notes.iter().map(|n| n.path.clone()).collect();
        let mut par_nom: HashMap<String, String> = HashMap::new();
        for n in &notes {
            par_nom
                .entry(nom_de_base(&n.path))
                .or_insert_with(|| n.path.clone());
        }

        let mut noeuds: BTreeMap<String, GraphNode> = BTreeMap::new();
        let mut aretes: BTreeSet<GraphEdge> = BTreeSet::new();
        let mut casses = Vec::new();
        let mut toutes_tags: BTreeSet<String> = BTreeSet::new();

        for note in &notes {
            let contenu = self.read_note(&note.path).unwrap_or_default();
            let tags = extraire_tags(&contenu);
            toutes_tags.extend(tags.iter().cloned());
            noeuds.insert(
                note.path.clone(),
                GraphNode {
                    id: note.path.clone(),
                    name: nom_de_base(&note.path),
                    folder: dossier(&note.path),
                    tags,
                    out_degree: 0,
                    in_degree: 0,
                },
            );

            for cible in extraire_liens(&contenu) {
                match resoudre(&cible, &par_chemin, &par_nom) {
                    // Un lien d'une note vers elle-même n'apprend rien et
                    // encombre le rendu.
                    Some(vers) if vers != note.path => {
                        aretes.insert(GraphEdge {
                            from: note.path.clone(),
                            to: vers,
                        });
                    }
                    Some(_) => {}
                    None => casses.push(BrokenLink {
                        from: note.path.clone(),
                        target: cible,
                    }),
                }
            }
        }

        for a in &aretes {
            if let Some(n) = noeuds.get_mut(&a.from) {
                n.out_degree += 1;
            }
            if let Some(n) = noeuds.get_mut(&a.to) {
                n.in_degree += 1;
            }
        }

        Ok(VaultGraph {
            nodes: noeuds.into_values().collect(),
            edges: aretes.into_iter().collect(),
            broken: casses,
            tags: toutes_tags.into_iter().collect(),
        })
    }
}

/// Encode une valeur pour un paramètre d'URL (RFC 3986).
///
/// Écrit à la main plutôt qu'ajouter une dépendance pour une seule fonction.
/// Les caractères non réservés passent tels quels, le reste en `%XX` —
/// indispensable pour un chemin qui contient `mémoire/` ou des espaces.
pub fn encoder_url(valeur: &str) -> String {
    let mut out = String::with_capacity(valeur.len());
    for octet in valeur.as_bytes() {
        match octet {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*octet as char)
            }
            _ => out.push_str(&format!("%{octet:02X}")),
        }
    }
    out
}

/// URI d'ouverture d'une note dans Obsidian.
///
/// `path` prend un chemin absolu : c'est la forme qui n'exige pas de connaître
/// le nom du coffre Obsidian parent, lequel dépend de l'endroit où l'on a
/// imbriqué `obsia_vault`.
pub fn uri_obsidian(chemin_absolu: &str) -> String {
    format!("obsidian://open?path={}", encoder_url(chemin_absolu))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== Retrait du code =====

    #[test]
    fn un_lien_dans_un_bloc_de_code_n_est_pas_un_lien() {
        // C'est exactement ce qu'on écrit dans la documentation d'un coffre :
        // le compter créerait des arêtes fantômes.
        let md = "Vrai [[note-a]]\n\n```\nExemple : [[note-fantome]]\n```\n";
        assert_eq!(extraire_liens(md), vec!["note-a"]);
    }

    #[test]
    fn un_lien_en_code_en_ligne_n_est_pas_un_lien() {
        assert_eq!(
            extraire_liens("écris `[[modele]]` puis [[vrai]]"),
            vec!["vrai"]
        );
    }

    #[test]
    fn un_bloc_non_ferme_neutralise_la_suite() {
        let md = "avant [[a]]\n```\n[[b]]\n[[c]]";
        assert_eq!(extraire_liens(md), vec!["a"]);
    }

    #[test]
    fn le_retrait_du_code_preserve_les_lignes() {
        // Sans quoi deux fragments se recolleraient en un faux lien.
        let nettoye = sans_code("a\n```\nx\n```\nb");
        assert_eq!(nettoye.lines().count(), 5);
    }

    // ===== Liens =====

    #[test]
    fn lit_les_formes_de_lien_interne() {
        let md = "[[simple]] [[avec|alias]] [[note#ancre]] [[dossier/note]] ![[inclusion]]";
        assert_eq!(
            extraire_liens(md),
            vec!["simple", "avec", "note", "dossier/note", "inclusion"]
        );
    }

    #[test]
    fn lit_les_liens_markdown_du_coffre() {
        let md = "[une note](IA/agents/assistant.md) et [le site](https://exemple.test)";
        assert_eq!(extraire_liens(md), vec!["IA/agents/assistant.md"]);
    }

    #[test]
    fn ignore_les_schemas_externes() {
        let md = "[a](https://x) [b](http://y) [c](mailto:z@x) [d](obsidian://open)";
        assert!(extraire_liens(md).is_empty());
    }

    #[test]
    fn un_lien_vide_est_ignore() {
        assert!(extraire_liens("[[]] [[ | ]] [[#ancre]]").is_empty());
    }

    #[test]
    fn un_crochet_non_ferme_ne_bloque_pas_l_analyse() {
        assert_eq!(extraire_liens("[[a]] puis [[jamais-ferme"), vec!["a"]);
    }

    // ===== Étiquettes =====

    #[test]
    fn lit_les_etiquettes() {
        assert_eq!(
            extraire_tags("Note sur #linux et #infra/proxmox."),
            vec!["infra/proxmox", "linux"]
        );
    }

    #[test]
    fn un_titre_n_est_pas_une_etiquette() {
        // Le « # » d'un titre est suivi d'une espace.
        assert!(extraire_tags("# Titre\n## Sous-titre\n").is_empty());
    }

    #[test]
    fn un_diese_colle_a_un_mot_n_est_pas_une_etiquette() {
        assert!(extraire_tags("couleur#fff et note#ancre").is_empty());
    }

    #[test]
    fn une_etiquette_purement_numerique_est_ignoree() {
        // « #1 » désigne une issue, pas une catégorie.
        assert!(extraire_tags("voir #1 et #42").is_empty());
        assert_eq!(extraire_tags("voir #v2"), vec!["v2"]);
    }

    #[test]
    fn les_etiquettes_sont_dedupliquees_et_triees() {
        assert_eq!(extraire_tags("#b #a #b"), vec!["a", "b"]);
    }

    // ===== Résolution =====

    fn index() -> (BTreeSet<String>, HashMap<String, String>) {
        let chemins: BTreeSet<String> = ["IA/agents/assistant.md", "mémoire/sommaire.md"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut noms = HashMap::new();
        noms.insert(
            "assistant".to_string(),
            "IA/agents/assistant.md".to_string(),
        );
        noms.insert("sommaire".to_string(), "mémoire/sommaire.md".to_string());
        (chemins, noms)
    }

    #[test]
    fn resout_par_nom_a_l_echelle_du_coffre() {
        // C'est ainsi qu'Obsidian procède, et le contrat garantit l'unicité.
        let (c, n) = index();
        assert_eq!(
            resoudre("assistant", &c, &n).as_deref(),
            Some("IA/agents/assistant.md")
        );
    }

    #[test]
    fn resout_par_chemin_avec_ou_sans_extension() {
        let (c, n) = index();
        assert_eq!(
            resoudre("IA/agents/assistant.md", &c, &n).as_deref(),
            Some("IA/agents/assistant.md")
        );
        assert_eq!(
            resoudre("IA/agents/assistant", &c, &n).as_deref(),
            Some("IA/agents/assistant.md")
        );
    }

    #[test]
    fn une_cible_inexistante_ne_resout_pas() {
        let (c, n) = index();
        assert_eq!(resoudre("fantome", &c, &n), None);
    }

    // ===== Graphe complet =====

    fn coffre(fichiers: &[(&str, &str)]) -> (tempfile::TempDir, VaultState) {
        let dir = tempfile::tempdir().unwrap();
        for (chemin, contenu) in fichiers {
            let complet = dir.path().join(chemin);
            std::fs::create_dir_all(complet.parent().unwrap()).unwrap();
            std::fs::write(&complet, contenu).unwrap();
        }
        let vault = VaultState::resolve(Some(dir.path().to_string_lossy().to_string())).unwrap();
        (dir, vault)
    }

    #[test]
    fn construit_noeuds_aretes_et_degres() {
        let (_d, v) = coffre(&[
            ("a.md", "je pointe vers [[b]] et [[c]]"),
            ("b.md", "je pointe vers [[c]]"),
            ("c.md", "je ne pointe nulle part"),
        ]);
        let g = v.graph().unwrap();
        assert_eq!(g.nodes.len(), 3);
        assert_eq!(g.edges.len(), 3);
        let c = g.nodes.iter().find(|n| n.name == "c").unwrap();
        assert_eq!(c.in_degree, 2);
        assert_eq!(c.out_degree, 0);
    }

    #[test]
    fn les_liens_casses_sont_rapportes_pas_perdus() {
        // Obsidian les montre aussi : ce sont des fautes de frappe ou des
        // notes à créer.
        let (_d, v) = coffre(&[("a.md", "vers [[nulle-part]]")]);
        let g = v.graph().unwrap();
        assert!(g.edges.is_empty());
        assert_eq!(g.broken.len(), 1);
        assert_eq!(g.broken[0].target, "nulle-part");
    }

    #[test]
    fn un_lien_vers_soi_meme_est_ecarte() {
        let (_d, v) = coffre(&[("a.md", "je parle de [[a]]")]);
        assert!(v.graph().unwrap().edges.is_empty());
    }

    #[test]
    fn un_lien_repete_ne_compte_qu_une_fois() {
        // Trois mentions de la même note ne font pas trois arêtes.
        let (_d, v) = coffre(&[("a.md", "[[b]] [[b]] [[b]]"), ("b.md", "")]);
        assert_eq!(v.graph().unwrap().edges.len(), 1);
    }

    #[test]
    fn le_dossier_de_tete_est_conserve_pour_le_rendu() {
        let (_d, v) = coffre(&[("IA/agents/a.md", ""), ("racine.md", "")]);
        let g = v.graph().unwrap();
        let dossiers: Vec<&str> = g.nodes.iter().map(|n| n.folder.as_str()).collect();
        assert!(dossiers.contains(&"IA"));
        assert!(dossiers.contains(&"."));
    }

    // ===== URI Obsidian =====

    #[test]
    fn encode_ce_qui_doit_l_etre() {
        assert_eq!(encoder_url("abc-1_2.3~"), "abc-1_2.3~");
        assert_eq!(
            encoder_url("/home/kevin/coffre"),
            "%2Fhome%2Fkevin%2Fcoffre"
        );
        assert_eq!(encoder_url("a b"), "a%20b");
    }

    #[test]
    fn encode_les_accents_du_coffre() {
        // « mémoire/ » est un dossier réel du coffre : sans encodage,
        // l'URI serait rejetée.
        assert_eq!(encoder_url("mémoire"), "m%C3%A9moire");
    }

    #[test]
    fn l_uri_obsidian_porte_un_chemin_absolu() {
        // La forme `path` évite d'avoir à deviner le nom du coffre parent.
        let uri = uri_obsidian("/home/kevin/OBSIA/obsia_vault/note.md");
        let valeur = uri
            .strip_prefix("obsidian://open?path=")
            .expect("préfixe attendu");
        // Seul le schéma garde ses barres ; le chemin est entièrement encodé,
        // sans quoi Obsidian le tronquerait au premier séparateur.
        assert!(!valeur.contains('/'), "chemin non encodé : {valeur}");
        assert!(valeur.contains("%2F"));
    }

    #[test]
    fn les_etiquettes_du_coffre_sont_rassemblees() {
        let (_d, v) = coffre(&[("a.md", "#linux"), ("b.md", "#linux #infra")]);
        assert_eq!(v.graph().unwrap().tags, vec!["infra", "linux"]);
    }
}
