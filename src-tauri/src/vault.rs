//! Sandbox du coffre (obsi_vault) : accès fichiers BORNÉ au coffre.
//!
//! Règles de sécurité :
//! - Toute résolution de chemin passe par [`VaultState::safe_join`] qui
//!   refuse `..` (path traversal) et les chemins absolus.
//! - Seuls les fichiers `.md` sont lisibles/écrivables (notes markdown).
//! - Écriture interdite dans les dossiers protégés (`IA/`, `mémoire/`,
//!   `scripts/`) et dans le code framework (`src/`, `src-tauri/`, ...).
//! - La racine du coffre est canonicalisée : un symlink ne peut pas
//!   sortir du coffre.

use serde::Serialize;
use std::path::{Component, Path, PathBuf};
use tracing::{info, warn};

/// Dossiers exclus de la LISTE des notes (code framework, artefacts, git).
const EXCLUDED_DIRS: &[&str] = &[
    "src",
    "src-tauri",
    "target",
    "scripts",
    ".git",
    ".github",
    "node_modules",
    "dist",
];

/// Dossiers protégés : lecture possible, ÉCRITURE refusée.
const PROTECTED_DIRS: &[&str] = &["IA", "mémoire", "scripts"];

/// Entrée de note du coffre, exposée au frontend via IPC.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultEntry {
    /// Chemin relatif au coffre (séparateurs `/`).
    pub path: String,
    /// Nom de fichier (dernier segment).
    pub name: String,
    /// Date de dernière modification (RFC 3339).
    pub modified: String,
}

/// État Tauri : racine canonicalisée du coffre.
pub struct VaultState {
    root: PathBuf,
}

impl VaultState {
    /// Résout et canonicalise la racine du coffre.
    ///
    /// Ordre de résolution :
    /// 1. Env var `OBSI_VAULT_PATH` (chemin configurable, prioritaire)
    /// 2. Racine passée par le config file (via `vault_path` de la config)
    /// 3. Défaut dev : `<cwd>/../obsi_vault` puis `<cwd>/obsi_vault`
    /// 4. Dernier recours : `~/obsi_vault`
    pub fn resolve(root_override: Option<String>) -> Result<Self, String> {
        let candidates: Vec<PathBuf> = if let Some(p) = root_override {
            vec![PathBuf::from(p)]
        } else if let Ok(p) = std::env::var("OBSI_VAULT_PATH") {
            vec![PathBuf::from(p)]
        } else {
            let cwd = std::env::current_dir().unwrap_or_default();
            vec![
                cwd.join("../obsi_vault"),
                cwd.join("obsi_vault"),
                dirs::home_dir()
                    .map(|h| h.join("obsi_vault"))
                    .unwrap_or_default(),
            ]
        };

        for candidate in candidates {
            if candidate.is_dir() {
                let canonical = candidate
                    .canonicalize()
                    .map_err(|e| format!("impossible de canonicaliser {}: {e}", candidate.display()))?;
                info!(vault = %canonical.display(), "coffre résolu");
                return Ok(Self { root: canonical });
            }
        }
        Err("coffre introuvable — configurez OBSI_VAULT_PATH ou la config vault_path (chemin attendu : obsi_vault/)".to_string())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// État "coffre non configuré" : toutes les opérations renvoient une
    /// erreur explicite (utilisé au setup si le coffre est introuvable).
    pub fn unavailable(message: String) -> Self {
        warn!(%message, "VaultState indisponible — coffre non configuré");
        Self { root: PathBuf::new() }
    }

    fn ensure_configured(&self) -> Result<(), String> {
        if self.root.as_os_str().is_empty() {
            return Err("coffre non configuré (OBSI_VAULT_PATH ou config vault_path requis)".into());
        }
        Ok(())
    }

    /// Liste les notes markdown du coffre (exclusion des dossiers framework).
    pub fn list_notes(&self) -> Result<Vec<VaultEntry>, String> {
        self.ensure_configured()?;
        let mut notes = Vec::new();
        self.walk(&self.root, &mut notes)?;
        notes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(notes)
    }

    fn walk(&self, dir: &Path, out: &mut Vec<VaultEntry>) -> Result<(), String> {
        let entries = std::fs::read_dir(dir)
            .map_err(|e| format!("lecture du coffre impossible: {e}"))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy().to_string();
            if path.is_dir() {
                if !EXCLUDED_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                    self.walk(&path, out)?;
                }
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                if let Ok(meta) = path.metadata() {
                    let rel = self
                        .to_relative(&path)
                        .unwrap_or_else(|| name.clone());
                    out.push(VaultEntry {
                        path: rel,
                        name,
                        modified: format_modified(meta.modified()),
                    });
                }
            }
        }
        Ok(())
    }

    /// Lit une note markdown (chemin relatif). Sandbox appliquée.
    pub fn read_note(&self, rel: &str) -> Result<String, String> {
        let path = self.safe_join(rel, true)?;
        std::fs::read_to_string(&path).map_err(|e| format!("lecture de {} impossible: {e}", path.display()))
    }

    /// Écrit une note markdown (chemin relatif). Sandbox + protection
    /// des dossiers protégés appliquées.
    pub fn write_note(&self, rel: &str, content: &str) -> Result<VaultEntry, String> {
        let path = self.safe_join(rel, false)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("création de {} impossible: {e}", parent.display()))?;
        }
        std::fs::write(&path, content)
            .map_err(|e| format!("écriture de {} impossible: {e}", path.display()))?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let meta = path.metadata().ok();
        let rel_path = self.to_relative(&path).unwrap_or(name.clone());
        info!(note = %rel_path, "note écrite");
        let modified = meta
            .and_then(|m| m.modified().ok())
            .map(|t| format_modified(Ok(t)))
            .unwrap_or_default();
        Ok(VaultEntry {
            path: rel_path,
            name,
            modified,
        })
    }

    /// Résout un chemin relatif en vérifiant :
    /// - pas de `..` ni de chemin absolu (anti path traversal)
    /// - extension `.md` uniquement
    /// - pour l'écriture : pas dans les dossiers protégés
    /// - le résultat (ou son parent) reste DANS la racine canonicalisée
    fn safe_join(&self, rel: &str, for_read: bool) -> Result<PathBuf, String> {
        self.ensure_configured()?;
        if rel.trim().is_empty() {
            return Err("chemin vide".into());
        }
        let rel_path = Path::new(rel);
        if rel_path.is_absolute() {
            return Err("chemin absolu refusé".into());
        }

        let mut resolved = self.root.clone();
        let mut first_component: Option<String> = None;
        for comp in rel_path.components() {
            match comp {
                Component::Normal(c) => {
                    if first_component.is_none() {
                        first_component = Some(c.to_string_lossy().to_string());
                    }
                    resolved.push(c);
                }
                Component::ParentDir => return Err("path traversal refusé (..)".into()),
                Component::RootDir | Component::Prefix(_) => {
                    return Err("chemin absolu refusé".into())
                }
                Component::CurDir => {}
            }
        }

        // Extension .md uniquement
        if resolved.extension().and_then(|e| e.to_str()) != Some("md") {
            return Err("seuls les fichiers .md sont accessibles dans le coffre".into());
        }

        // Dossiers protégés : écriture refusée
        if !for_read {
            if let Some(first) = &first_component {
                if PROTECTED_DIRS.contains(&first.as_str()) {
                    return Err(format!("dossier protégé, écriture refusée : {first}"));
                }
            }
        }

        // Vérification sandbox : le chemin (ou son parent s'il n'existe pas
        // encore) doit rester sous la racine canonicalisée.
        let check = if resolved.exists() {
            resolved
                .canonicalize()
                .map_err(|e| format!("canonicalize impossible: {e}"))?
        } else {
            resolved
                .parent()
                .and_then(|p| p.canonicalize().ok())
                .unwrap_or_else(|| self.root.clone())
        };
        if !check.starts_with(&self.root) {
            warn!(path = %resolved.display(), "tentative de sortie du coffre bloquée");
            return Err("chemin hors du coffre refusé".into());
        }

        Ok(resolved)
    }

    /// Convertit un chemin absolu en chemin relatif au coffre.
    fn to_relative(&self, path: &Path) -> Option<String> {
        path.strip_prefix(&self.root)
            .ok()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
    }
}

fn format_modified(modified: std::io::Result<std::time::SystemTime>) -> String {
    match modified {
        Ok(t) => {
            let secs = t
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            format!("{}", secs)
        }
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_vault() -> (tempfile::TempDir, VaultState) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("IA")).unwrap();
        fs::create_dir_all(root.join("notes")).unwrap();
        fs::create_dir_all(root.join("src-tauri")).unwrap();
        fs::write(root.join("note1.md"), "# Note 1").unwrap();
        fs::write(root.join("IA/prompt.md"), "# Prompt").unwrap();
        fs::write(root.join("notes/sub.md"), "# Sub").unwrap();
        fs::write(root.join("src-tauri/main.rs"), "fn main() {}").unwrap();
        fs::write(root.join("secret.txt"), "pas un md").unwrap();
        let state = VaultState {
            root: root.canonicalize().unwrap(),
        };
        (dir, state)
    }

    #[test]
    fn list_filtre_les_notes() {
        let (_tmp, vault) = temp_vault();
        let notes = vault.list_notes().unwrap();
        let paths: Vec<String> = notes.iter().map(|n| n.path.clone()).collect();
        assert!(paths.contains(&"note1.md".to_string()));
        assert!(paths.contains(&"IA/prompt.md".to_string()));
        assert!(paths.contains(&"notes/sub.md".to_string()));
        // Le framework et les non-md sont exclus
        assert!(!paths.iter().any(|p| p.contains("src-tauri")));
        assert!(!paths.iter().any(|p| p.ends_with(".txt")));
    }

    #[test]
    fn read_ok() {
        let (_tmp, vault) = temp_vault();
        assert_eq!(vault.read_note("note1.md").unwrap(), "# Note 1");
    }

    #[test]
    fn write_ok_et_list() {
        let (_tmp, vault) = temp_vault();
        let entry = vault.write_note("notes/nouvelle.md", "# Nouvelle").unwrap();
        assert_eq!(entry.path, "notes/nouvelle.md");
        assert_eq!(vault.read_note("notes/nouvelle.md").unwrap(), "# Nouvelle");
    }

    #[test]
    fn path_traversal_refuse() {
        let (_tmp, vault) = temp_vault();
        assert!(vault.read_note("../outside.md").is_err());
        assert!(vault.read_note("/etc/passwd").is_err());
        assert!(vault.read_note("notes/../../outside.md").is_err());
    }

    #[test]
    fn extension_non_md_refusee() {
        let (_tmp, vault) = temp_vault();
        assert!(vault.read_note("secret.txt").is_err());
        assert!(vault.write_note("notes/script.sh", "#!/bin/sh").is_err());
    }

    #[test]
    fn ecriture_dossier_protege_refusee() {
        let (_tmp, vault) = temp_vault();
        assert!(vault.write_note("IA/prompt.md", "# modifié").is_err());
        assert!(vault.write_note("scripts/x.md", "x").is_err());
        // Lecture OK en revanche
        assert_eq!(vault.read_note("IA/prompt.md").unwrap(), "# Prompt");
    }
}
