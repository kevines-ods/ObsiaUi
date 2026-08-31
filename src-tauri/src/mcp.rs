//! Outils MCP déclarés dans le coffre (`IA/MCP/`).
//!
//! ObsiaUi **ne se connecte pas** aux serveurs MCP : le coffre les déclare, et
//! c'est le harness qui les branche. Ce module se contente de les lire et de
//! dire quels agents s'en servent — ce qui suffit à répondre à la seule
//! question qu'on se pose devant la liste : « à quoi sert celui-là, et qui
//! l'utilise ? »
//!
//! La lecture est **tolérante** : un fichier sans frontmatter est listé avec
//! son nom de fichier plutôt qu'écarté. Refuser d'afficher un outil parce
//! qu'il manque une ligne de métadonnées n'aiderait personne.

use crate::vault::VaultState;
use serde::{Deserialize, Serialize};
use tracing::warn;

/// Dossier des déclarations MCP, relatif au coffre.
pub const MCP_DIR: &str = "IA/MCP";

/// Un outil MCP déclaré.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpInfo {
    /// Chemin relatif au coffre (ex. `IA/MCP/git-hub.md`).
    pub path: String,
    pub name: String,
    pub description: String,
    /// Agents qui le déclarent dans leur frontmatter.
    pub declared_by: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawMcp {
    name: Option<String>,
    description: Option<String>,
}

/// Nom et description d'une déclaration, avec repli sur le nom de fichier.
pub fn parse_mcp(raw: &str, fichier: &str) -> (String, String) {
    let defaut = fichier.trim_end_matches(".md").to_string();
    let Ok((fm, _)) = crate::agents::split_frontmatter(raw) else {
        return (defaut, String::new());
    };
    let champs: RawMcp = serde_yaml::from_str(fm).unwrap_or_default();
    (
        champs
            .name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or(defaut),
        champs.description.unwrap_or_default(),
    )
}

impl VaultState {
    /// Liste les outils MCP du coffre, triés par nom.
    pub fn mcp_list(&self) -> Result<Vec<McpInfo>, String> {
        self.ensure_configured()?;
        let dir = self.root().join(MCP_DIR);
        if !dir.is_dir() {
            return Ok(Vec::new());
        }

        // Qui déclare quoi : une seule lecture des agents, puis un index.
        let agents = self.agents_list().unwrap_or_default();

        let entries = std::fs::read_dir(&dir)
            .map_err(|e| format!("lecture de {} impossible: {e}", dir.display()))?;
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let fichier = entry.file_name().to_string_lossy().to_string();
            let rel = self
                .to_relative(&path)
                .unwrap_or_else(|| format!("{MCP_DIR}/{fichier}"));
            let raw = match std::fs::read_to_string(&path) {
                Ok(raw) => raw,
                Err(e) => {
                    warn!(path = %rel, %e, "déclaration MCP illisible, ignorée");
                    continue;
                }
            };
            let (name, description) = parse_mcp(&raw, &fichier);
            let declared_by = agents
                .iter()
                .filter(|a| a.mcp.iter().any(|m| m == &name))
                .map(|a| a.name.clone())
                .collect();
            out.push(McpInfo {
                path: rel,
                name,
                description,
                declared_by,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Rédige une déclaration MCP dans `brouillon/`.
    ///
    /// Jamais directement dans `IA/MCP/` : le coffre est en lecture seule hors
    /// `brouillon/`, et le contrat impose un patch relu pour toute
    /// modification durable (§2). Un outil ajouté sans relecture donnerait à
    /// des agents un accès que personne n'a validé.
    pub fn mcp_draft(&self, name: &str, description: &str, body: &str) -> Result<String, String> {
        let nom = name.trim();
        if nom.is_empty() {
            return Err("nom requis".into());
        }
        let slug = crate::session::slug(nom);
        let chemin = format!("brouillon/IA/MCP/{slug}.md");
        let contenu = format!(
            "---\nschema: 1\nkind: mcp\nname: {slug}\ndescription: {}\nread_only: true\n---\n\n\
             # {nom}\n\n{}\n",
            description.trim().replace('\n', " "),
            body.trim()
        );
        self.write_note(&chemin, &contenu)?;
        Ok(chemin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn coffre() -> (tempfile::TempDir, VaultState) {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("brouillon")).unwrap();
        let state = VaultState::pour_tests(dir.path().canonicalize().unwrap());
        (dir, state)
    }

    #[test]
    fn lit_le_nom_et_la_description_du_frontmatter() {
        let raw = "---\nschema: 1\nkind: mcp\nname: git-hub\ndescription: Accès aux dépôts.\n---\n\n# GitHub\n";
        assert_eq!(
            parse_mcp(raw, "git-hub.md"),
            ("git-hub".into(), "Accès aux dépôts.".into())
        );
    }

    #[test]
    fn un_fichier_sans_frontmatter_reste_liste() {
        // Refuser d'afficher un outil pour une ligne de métadonnées manquante
        // n'aide personne.
        let (nom, desc) = parse_mcp("# Chrome DevTools\n\nDes outils.", "chrome-devtools.md");
        assert_eq!(nom, "chrome-devtools");
        assert!(desc.is_empty());
    }

    #[test]
    fn un_frontmatter_illisible_retombe_sur_le_nom_de_fichier() {
        let (nom, _) = parse_mcp("---\n: : :\n---\n", "outil.md");
        assert_eq!(nom, "outil");
    }

    #[test]
    fn un_nom_vide_ne_remplace_pas_le_nom_de_fichier() {
        let (nom, _) = parse_mcp("---\nname: \"  \"\n---\n", "vrai-nom.md");
        assert_eq!(nom, "vrai-nom");
    }

    // ===== Rédaction =====

    #[test]
    fn une_declaration_part_en_brouillon_jamais_dans_ia() {
        // C'est la garantie qui compte : un outil MCP donne des accès, et le
        // contrat veut qu'ils soient relus avant d'entrer dans le coffre.
        let (dir, coffre) = coffre();
        let chemin = coffre
            .mcp_draft("Chrome DevTools", "Pilote un navigateur.", "npx serveur")
            .unwrap();
        assert_eq!(chemin, "brouillon/IA/MCP/chrome-devtools.md");
        assert!(dir.path().join(&chemin).is_file());
        assert!(!dir.path().join("IA/MCP/chrome-devtools.md").exists());

        let ecrit = fs::read_to_string(dir.path().join(&chemin)).unwrap();
        assert!(ecrit.contains("kind: mcp"));
        assert!(ecrit.contains("Pilote un navigateur."));
        assert!(ecrit.contains("npx serveur"));
    }

    #[test]
    fn une_description_multiligne_ne_casse_pas_le_frontmatter() {
        // Un saut de ligne dans une valeur YAML non citée coupe le document :
        // la note serait listée sans nom ni description.
        let (dir, coffre) = coffre();
        let chemin = coffre
            .mcp_draft("outil", "première ligne\nseconde ligne", "")
            .unwrap();
        let ecrit = fs::read_to_string(dir.path().join(&chemin)).unwrap();
        let (nom, desc) = parse_mcp(&ecrit, "outil.md");
        assert_eq!(nom, "outil");
        assert_eq!(desc, "première ligne seconde ligne");
    }

    #[test]
    fn un_nom_vide_est_refuse() {
        let (_dir, coffre) = coffre();
        assert!(coffre.mcp_draft("   ", "", "").is_err());
    }
}
