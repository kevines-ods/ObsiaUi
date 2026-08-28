use crate::llm::provider::{LlmError, LlmProvider, ModelCapability, ModelInfo};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

#[derive(Clone)]
pub struct ModelRegistry {
    models: Arc<RwLock<HashMap<String, ModelInfo>>>,
    local_model_dirs: Vec<PathBuf>,
}

impl ModelRegistry {
    pub fn new(local_model_dirs: Vec<PathBuf>) -> Self {
        Self {
            models: Arc::new(RwLock::new(HashMap::new())),
            local_model_dirs,
        }
    }

    pub async fn register(&self, model: ModelInfo) {
        let mut models = self.models.write().await;
        models.insert(model.id.clone(), model);
    }

    pub async fn unregister(&self, model_id: &str) -> Option<ModelInfo> {
        let mut models = self.models.write().await;
        models.remove(model_id)
    }

    pub async fn get(&self, model_id: &str) -> Option<ModelInfo> {
        let models = self.models.read().await;
        models.get(model_id).cloned()
    }

    pub async fn list_all(&self) -> Vec<ModelInfo> {
        let models = self.models.read().await;
        models.values().cloned().collect()
    }

    pub async fn list_by_provider(&self, provider: &str) -> Vec<ModelInfo> {
        let models = self.models.read().await;
        models
            .values()
            .filter(|m| m.provider == provider)
            .cloned()
            .collect()
    }

    pub async fn list_by_capability(&self, capability: ModelCapability) -> Vec<ModelInfo> {
        let models = self.models.read().await;
        models
            .values()
            .filter(|m| m.capabilities.contains(&capability))
            .cloned()
            .collect()
    }

    pub async fn scan_local_models(&self) -> Result<usize, LlmError> {
        let mut count = 0;
        for dir in &self.local_model_dirs {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        if let Some(ext) = path.extension() {
                            if ext == "gguf" || ext == "safetensors" {
                                if let Some(model_info) = self.parse_local_model(&path).await {
                                    self.register(model_info).await;
                                    count += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        info!("Scanned local models: {} found", count);
        Ok(count)
    }

    async fn parse_local_model(&self, path: &Path) -> Option<ModelInfo> {
        let file_name = path.file_stem()?.to_str()?;
        let ext = path.extension()?.to_str()?;

        let (context_window, capabilities) = match ext {
            "gguf" => (4096, vec![ModelCapability::Chat]),
            "safetensors" => (
                8192,
                vec![ModelCapability::Chat, ModelCapability::Embedding],
            ),
            _ => return None,
        };

        Some(ModelInfo {
            id: format!("local/{}", file_name),
            name: file_name.to_string(),
            provider: "local".to_string(),
            context_window,
            capabilities,
            pricing: None,
            local_path: Some(path.to_string_lossy().to_string()),
        })
    }
}

pub struct ProviderRegistry {
    providers: Arc<RwLock<HashMap<String, Arc<dyn LlmProvider>>>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register(&self, provider: Arc<dyn LlmProvider>) {
        let mut providers = self.providers.write().await;
        providers.insert(provider.id().to_string(), provider);
    }

    pub async fn get(&self, id: &str) -> Option<Arc<dyn LlmProvider>> {
        let providers = self.providers.read().await;
        providers.get(id).cloned()
    }

    pub async fn list_all(&self) -> Vec<Arc<dyn LlmProvider>> {
        let providers = self.providers.read().await;
        providers.values().cloned().collect()
    }

    pub async fn list_ids(&self) -> Vec<String> {
        let providers = self.providers.read().await;
        providers.keys().cloned().collect()
    }

    pub async fn health_check_all(&self) -> HashMap<String, Result<(), LlmError>> {
        let providers = self.providers.read().await;
        let mut results = HashMap::new();
        for (id, provider) in providers.iter() {
            results.insert(id.clone(), provider.health_check().await);
        }
        results
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}
