//! Agents du coffre : lecture + validation du frontmatter (schema 1).
//!
//! Le frontmatter est la **vérité machine** (cf. `VAULT-CONTRACT.md` §5) :
//! c'est le backend qui parse et valide, jamais l'UI. Les fichiers invalides
//! sont ignorés avec un warning (un agent cassé ne casse pas le sélecteur),
//! mais l'UI ne reçoit que des agents conformes.

use crate::vault::VaultState;
use serde::{Deserialize, Serialize};
use tracing::warn;

/// Dossier des agents, relatif au coffre.
pub const AGENTS_DIR: &str = "IA/agents";

/// Métadonnées d'un agent, parsées et validées depuis le frontmatter.
#[derive(Debug, Clone, Serialize)]
pub struct AgentInfo {
    /// Chemin relatif au coffre (ex. `IA/agents/assistant.md`).
    pub path: String,
    pub name: String,
    pub description: String,
    pub skills: Vec<String>,
    pub mcp: Vec<String>,
    pub read_only: bool,
}

/// Agent complet : métadonnées + corps (le system prompt, sans frontmatter).
#[derive(Debug, Clone, Serialize)]
pub struct AgentDoc {
    #[serde(flatten)]
    pub info: AgentInfo,
    /// Corps markdown de l'agent (sans le frontmatter YAML).
    pub content: String,
}

impl VaultState {
    /// Liste les agents du coffre : scan `IA/agents/*.md`, parse et valide le
    /// frontmatter (`schema >= 1`, `kind == "agent"`, `name`/`description`
    /// obligatoires). Tri par nom.
    pub fn agents_list(&self) -> Result<Vec<AgentInfo>, String> {
        self.ensure_configured()?;
        let dir = self.root().join(AGENTS_DIR);
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut agents = Vec::new();
        let entries = std::fs::read_dir(&dir)
            .map_err(|e| format!("lecture de {} impossible: {e}", dir.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let rel = self
                .to_relative(&path)
                .unwrap_or_else(|| path.to_string_lossy().to_string());
            let raw = match std::fs::read_to_string(&path) {
                Ok(raw) => raw,
                Err(e) => {
                    warn!(path = %rel, %e, "agent illisible, ignoré");
                    continue;
                }
            };
            match parse_agent(&raw, &rel) {
                Ok(info) => agents.push(info),
                Err(e) => warn!(path = %rel, %e, "agent ignoré : frontmatter invalide"),
            }
        }
        agents.sort_by(|a, b| a.name.cmp(&b.name));
        info_agents(&agents);
        Ok(agents)
    }

    /// Lit un agent complet. `path` accepte soit le chemin complet
    /// (`IA/agents/assistant.md`), soit le simple nom de fichier
    /// (`assistant.md`). Sandbox + confinement à `IA/agents/` appliqués.
    pub fn agent_read(&self, path: &str) -> Result<AgentDoc, String> {
        self.ensure_configured()?;
        let rel = if !path.contains('/') {
            format!("{AGENTS_DIR}/{path}")
        } else {
            path.to_string()
        };
        let full = self.safe_join(&rel, true)?;
        if !full.starts_with(self.root().join(AGENTS_DIR)) {
            return Err("accès refusé : chemin hors de IA/agents/".into());
        }
        let raw = std::fs::read_to_string(&full)
            .map_err(|e| format!("lecture de {} impossible: {e}", full.display()))?;
        let info = parse_agent(&raw, &rel)?;
        let (_, body) = split_frontmatter(&raw)?;
        Ok(AgentDoc {
            info,
            content: body.trim().to_string(),
        })
    }
}

fn info_agents(agents: &[AgentInfo]) {
    if !agents.is_empty() {
        warn!(count = agents.len(), "agents chargés (frontmatter validé)");
    }
}

/// Découpe un fichier markdown en `(frontmatter_yaml, corps)`.
/// Le fichier doit commencer par `---` et contenir une ligne `---` fermante.
fn split_frontmatter(raw: &str) -> Result<(&str, &str), String> {
    let rest = raw
        .strip_prefix("---")
        .ok_or("pas de frontmatter (doit commencer par ---)")?;
    let end = rest
        .find("\n---")
        .ok_or("frontmatter non fermé (ligne --- manquante)")?;
    let fm = &rest[..end];
    let body = &rest[end + 4..];
    Ok((fm, body))
}

/// Structure brute du frontmatter, relâchée (tous les champs optionnels).
#[derive(Debug, Deserialize)]
struct RawFrontmatter {
    schema: Option<i64>,
    kind: Option<String>,
    name: Option<String>,
    description: Option<String>,
    skills: Option<Vec<String>>,
    mcp: Option<Vec<String>>,
    read_only: Option<bool>,
}

/// Parse et valide le frontmatter d'un agent (schema 1, kind == "agent").
fn parse_agent(raw: &str, rel: &str) -> Result<AgentInfo, String> {
    let (fm, _) = split_frontmatter(raw)?;
    let raw_fm: RawFrontmatter =
        serde_yaml::from_str(fm).map_err(|e| format!("YAML invalide dans le frontmatter: {e}"))?;
    if raw_fm.kind.as_deref() != Some("agent") {
        return Err("kind != agent — ce n'est pas un agent".into());
    }
    if raw_fm.schema.unwrap_or(0) < 1 {
        return Err("schema < 1 — format non supporté".into());
    }
    let name = raw_fm
        .name
        .filter(|n| !n.trim().is_empty())
        .ok_or("name manquant ou vide")?;
    let description = raw_fm
        .description
        .filter(|d| !d.trim().is_empty())
        .ok_or("description manquante ou vide")?;
    Ok(AgentInfo {
        path: rel.to_string(),
        name,
        description,
        skills: raw_fm.skills.unwrap_or_default(),
        mcp: raw_fm.mcp.unwrap_or_default(),
        read_only: raw_fm.read_only.unwrap_or(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::VaultState;
    use std::fs;

    const VALID_AGENT: &str = r#"---
schema: 1
kind: agent
name: assistant
description: Agent de base de l'app OBSIA.
skills:
  - obsidian-manager
  - mermaid
mcp:
  - git-hub
read_only: false
---

# Assistant

## Rôle

Orchestre le coffre et modifie l'UI.
"#;

    fn temp_vault() -> (tempfile::TempDir, VaultState) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("IA/agents")).unwrap();
        fs::create_dir_all(root.join("IA/skills")).unwrap();
        fs::create_dir_all(root.join("IA/notes")).unwrap();
        fs::write(root.join("IA/agents/assistant.md"), VALID_AGENT).unwrap();
        fs::write(
            root.join("IA/agents/casse.md"),
            "# Pas de frontmatter du tout\n",
        )
        .unwrap();
        fs::write(
            root.join("IA/agents/autre.md"),
            "---\nschema: 1\nkind: skill\nname: x\n---\n",
        )
        .unwrap();
        fs::write(root.join("IA/notes/note.md"), "# note hors agents").unwrap();
        let state = VaultState::resolve(Some(root.to_string_lossy().to_string())).unwrap();
        (dir, state)
    }

    #[test]
    fn agents_list_ne_retourne_que_les_valides() {
        let (_tmp, vault) = temp_vault();
        let agents = vault.agents_list().unwrap();
        assert_eq!(agents.len(), 1, "seul assistant.md est un agent valide");
        let a = &agents[0];
        assert_eq!(a.name, "assistant");
        assert_eq!(a.path, "IA/agents/assistant.md");
        assert_eq!(a.skills, vec!["obsidian-manager", "mermaid"]);
        assert_eq!(a.mcp, vec!["git-hub"]);
        assert!(!a.read_only);
        assert!(!a.description.is_empty());
    }

    #[test]
    fn agent_read_par_nom_et_par_chemin() {
        let (_tmp, vault) = temp_vault();
        let doc = vault.agent_read("assistant.md").unwrap();
        assert_eq!(doc.info.name, "assistant");
        assert!(doc.content.contains("## Rôle"), "corps sans frontmatter");
        assert!(!doc.content.contains("schema:"), "le frontmatter est exclu");

        let doc2 = vault.agent_read("IA/agents/assistant.md").unwrap();
        assert_eq!(doc2.content, doc.content);
    }

    #[test]
    fn agent_read_refuse_hors_agents() {
        let (_tmp, vault) = temp_vault();
        assert!(vault.agent_read("../secret.md").is_err());
        assert!(vault.agent_read("IA/notes/note.md").is_err());
        assert!(
            vault.agent_read("casse.md").is_err(),
            "frontmatter invalide"
        );
    }

    #[test]
    fn split_frontmatter_decoupe_proprement() {
        let (fm, body) = split_frontmatter(VALID_AGENT).unwrap();
        assert!(fm.contains("schema: 1"));
        assert!(body.trim().starts_with("# Assistant"));
    }

    #[test]
    fn parse_rejette_mauvais_kind_et_schema() {
        let rel = "IA/agents/x.md";
        let mauvais_kind = "---\nschema: 1\nkind: skill\nname: x\n---\n# X\n";
        assert!(parse_agent(mauvais_kind, rel).is_err());

        let vieux_schema = "---\nschema: 0\nkind: agent\nname: x\ndescription: d\n---\n";
        assert!(parse_agent(vieux_schema, rel).is_err());
    }
}
