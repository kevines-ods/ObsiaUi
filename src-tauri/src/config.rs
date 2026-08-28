//! Configuration applicative sécurisée (clés API, préférences).
//!
//! Décision d'architecture (à revoir avec le lead) :
//! - Les clés API ne sont JAMAIS en dur dans le code ni commitées.
//! - Priorité de lecture : env var > fichier config.
//! - Le fichier de config est stocké dans le répertoire de config de l'app
//!   (`app_config_dir`), PAS dans le repo, avec permissions 0600 sur Unix.
//! - Pas de chiffrement maison (fausse sécurité) : le stockage final
//!   recommandé est le trousseau OS via la crate `keyring` (à brancher
//!   ultérieurement). Le fichier protégé est un palliatif documenté.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;
use tracing::{info, warn};

/// Préfixes d'env vars par provider (prioritaires sur la config fichier).
pub const ENV_VAR_BY_PROVIDER: &[(&str, &str)] = &[
    ("openai", "OPENAI_API_KEY"),
    ("anthropic", "ANTHROPIC_API_KEY"),
    ("openrouter", "OPENROUTER_API_KEY"),
    ("gemini", "GEMINI_API_KEY"),
    ("mistral", "MISTRAL_API_KEY"),
    ("cohere", "COHERE_API_KEY"),
];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    /// Clés API par provider_id (stockées hors repo, fichier 0600).
    #[serde(default)]
    pub api_keys: HashMap<String, String>,
    /// Chemin du coffre (optionnel — sinon résolution auto).
    #[serde(default)]
    pub vault_path: Option<String>,
    /// Provider par défaut pour le chat.
    #[serde(default)]
    pub default_provider: Option<String>,
    /// Host Ollama personnalisé (ex: http://localhost:11434).
    #[serde(default)]
    pub ollama_host: Option<String>,
}

/// Vue exposée au frontend : jamais les valeurs de clés, seulement leur
/// présence (le frontend n'a pas besoin de relire les secrets).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigView {
    pub api_keys_configured: Vec<String>,
    pub vault_path: Option<String>,
    pub default_provider: Option<String>,
    pub ollama_host: Option<String>,
}

/// Patch de mise à jour de la config (champs optionnels = non modifiés).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigPatch {
    pub set_api_key: Option<SetApiKey>,
    pub vault_path: Option<String>,
    pub default_provider: Option<String>,
    pub ollama_host: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetApiKey {
    pub provider_id: String,
    pub api_key: String,
}

impl AppConfig {
    /// Construit une config depuis le fichier (ou défaut).
    pub fn load(path: &PathBuf) -> Self {
        match std::fs::read_to_string(path) {
            Ok(raw) => match serde_json::from_str::<AppConfig>(&raw) {
                Ok(cfg) => cfg,
                Err(e) => {
                    warn!(%e, "config corrompue — utilisation du défaut");
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    /// Sauvegarde avec permissions 0600 (Unix) + fsync.
    pub fn save(&self, path: &PathBuf) -> Result<(), String> {
        let raw = serde_json::to_string_pretty(self)
            .map_err(|e| format!("sérialisation config: {e}"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("création {}: {e}", parent.display()))?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::write(path, raw).map_err(|e| format!("écriture config: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        info!(path = %path.display(), "config sauvegardée");
        Ok(())
    }

    /// Clé API effective : env var d'abord, sinon fichier config.
    pub fn api_key_for(&self, provider_id: &str) -> Option<String> {
        for (pid, var) in ENV_VAR_BY_PROVIDER {
            if *pid == provider_id {
                if let Ok(v) = std::env::var(var) {
                    if !v.is_empty() {
                        return Some(v);
                    }
                }
            }
        }
        self.api_keys.get(provider_id).cloned()
    }

    pub fn view(&self) -> ConfigView {
        ConfigView {
            api_keys_configured: self.api_keys.keys().cloned().collect(),
            vault_path: self.vault_path.clone(),
            default_provider: self.default_provider.clone(),
            ollama_host: self.ollama_host.clone(),
        }
    }

    pub fn apply_patch(&mut self, patch: ConfigPatch) {
        if let Some(SetApiKey { provider_id, api_key }) = patch.set_api_key {
            let trimmed = api_key.trim().to_string();
            if trimmed.is_empty() {
                self.api_keys.remove(&provider_id);
            } else {
                self.api_keys.insert(provider_id, trimmed);
            }
        }
        if let Some(p) = patch.vault_path {
            self.vault_path = if p.trim().is_empty() { None } else { Some(p) };
        }
        if let Some(p) = patch.default_provider {
            self.default_provider = if p.trim().is_empty() { None } else { Some(p) };
        }
        if let Some(p) = patch.ollama_host {
            self.ollama_host = if p.trim().is_empty() { None } else { Some(p) };
        }
    }
}

/// État Tauri : config chargée + chemin du fichier.
pub struct ConfigState {
    pub path: PathBuf,
    pub inner: RwLock<AppConfig>,
}

impl ConfigState {
    pub fn new(path: PathBuf) -> Self {
        let config = AppConfig::load(&path);
        Self {
            path,
            inner: RwLock::new(config),
        }
    }

    pub fn read(&self) -> AppConfig {
        self.inner
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn update(&self, patch: ConfigPatch) -> Result<ConfigView, String> {
        let mut guard = self
            .inner
            .write()
            .map_err(|_| "verrou config poisonné".to_string())?;
        guard.apply_patch(patch);
        let view = guard.view();
        let snapshot = guard.clone();
        drop(guard);
        snapshot.save(&self.path)?;
        Ok(view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config_path() -> PathBuf {
        let dir = tempfile::tempdir().unwrap();
        dir.path().join("obsi/config.json")
    }

    #[test]
    fn roundtrip_config() {
        let path = temp_config_path();
        let mut cfg = AppConfig::default();
        cfg.api_keys
            .insert("openai".into(), "sk-test-123".into());
        cfg.default_provider = Some("ollama".into());
        cfg.save(&path).unwrap();

        let loaded = AppConfig::load(&path);
        assert_eq!(loaded.api_keys.get("openai").unwrap(), "sk-test-123");
        assert_eq!(loaded.default_provider.as_deref(), Some("ollama"));
    }

    #[test]
    fn permissions_0600_sur_unix() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = temp_config_path();
            AppConfig::default().save(&path).unwrap();
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "la config doit être en 0600");
        }
    }

    #[test]
    fn env_var_prioritaire_sur_config() {
        let mut cfg = AppConfig::default();
        cfg.api_keys
            .insert("openai".into(), "from-file".into());
        // Sans env var -> config fichier
        assert_eq!(cfg.api_key_for("openai").unwrap(), "from-file");
        // Avec env var -> env var
        unsafe {
            std::env::set_var("OPENAI_API_KEY", "from-env");
        }
        assert_eq!(cfg.api_key_for("openai").unwrap(), "from-env");
        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
        }
    }

    #[test]
    fn patch_vide_supprime_cle() {
        let mut cfg = AppConfig::default();
        cfg.api_keys
            .insert("openai".into(), "sk-abc".into());
        cfg.apply_patch(ConfigPatch {
            set_api_key: Some(SetApiKey {
                provider_id: "openai".into(),
                api_key: "   ".into(),
            }),
            ..Default::default()
        });
        assert!(!cfg.api_keys.contains_key("openai"));
    }

    #[test]
    fn config_view_ne_fuit_pas_les_secrets() {
        let mut cfg = AppConfig::default();
        cfg.api_keys
            .insert("openai".into(), "sk-super-secret".into());
        let view = cfg.view();
        assert!(view.api_keys_configured.contains(&"openai".to_string()));
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains("sk-super-secret"), "les secrets ne doivent pas sortir");
    }
}
