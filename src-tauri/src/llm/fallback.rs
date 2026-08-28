use crate::llm::provider::{ChatRequest, ChatResponse, LlmError, LlmProvider, TokenStream};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tracing::warn;

pub struct ProviderPool {
    providers: Arc<RwLock<Vec<Arc<dyn LlmProvider>>>>,
    strategy: PoolStrategy,
    circuit_breakers: Arc<RwLock<std::collections::HashMap<String, CircuitBreaker>>>,
}

#[derive(Clone, Copy, Debug)]
pub enum PoolStrategy {
    RoundRobin,
    Priority,
    Fallback,
}

struct CircuitBreaker {
    failures: u32,
    last_failure: Option<Instant>,
    state: CircuitState,
    threshold: u32,
    timeout: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

impl ProviderPool {
    pub fn new(strategy: PoolStrategy) -> Self {
        Self {
            providers: Arc::new(RwLock::new(Vec::new())),
            strategy,
            circuit_breakers: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    pub async fn add_provider(&self, provider: Arc<dyn LlmProvider>) {
        let mut providers = self.providers.write().await;
        providers.push(provider);
    }

    pub async fn get_available(&self) -> Vec<Arc<dyn LlmProvider>> {
        let providers = self.providers.read().await;
        let breakers = self.circuit_breakers.read().await;
        providers
            .iter()
            .filter(|p| {
                breakers
                    .get(p.id())
                    .is_none_or(|cb| cb.state != CircuitState::Open)
            })
            .cloned()
            .collect()
    }

    async fn record_success(&self, provider_id: &str) {
        let mut breakers = self.circuit_breakers.write().await;
        if let Some(cb) = breakers.get_mut(provider_id) {
            cb.failures = 0;
            cb.state = CircuitState::Closed;
        }
    }

    async fn record_failure(&self, provider_id: &str) {
        let mut breakers = self.circuit_breakers.write().await;
        let cb = breakers
            .entry(provider_id.to_string())
            .or_insert(CircuitBreaker {
                failures: 0,
                last_failure: None,
                state: CircuitState::Closed,
                threshold: 5,
                timeout: Duration::from_secs(30),
            });
        cb.failures += 1;
        cb.last_failure = Some(Instant::now());
        if cb.failures >= cb.threshold {
            cb.state = CircuitState::Open;
            warn!("Circuit breaker OPEN for provider: {}", provider_id);
        }
    }

    pub async fn chat_with_fallback(&self, req: ChatRequest) -> Result<ChatResponse, LlmError> {
        let providers = self.get_available().await;
        if providers.is_empty() {
            return Err(LlmError::ProviderUnavailable(
                "No available providers".into(),
            ));
        }

        let mut last_error = None;
        for provider in providers {
            match provider.chat(req.clone()).await {
                Ok(resp) => {
                    self.record_success(provider.id()).await;
                    return Ok(resp);
                }
                Err(e) => {
                    self.record_failure(provider.id()).await;
                    last_error = Some(e);
                    warn!(
                        "Provider {} failed: {:?}, trying next",
                        provider.id(),
                        last_error
                    );
                }
            }
        }
        Err(last_error.unwrap_or(LlmError::ProviderUnavailable("All providers failed".into())))
    }

    pub async fn chat_stream_with_fallback(
        &self,
        req: ChatRequest,
    ) -> Result<TokenStream, LlmError> {
        let providers = self.get_available().await;
        if providers.is_empty() {
            return Err(LlmError::ProviderUnavailable(
                "No available providers".into(),
            ));
        }

        let mut last_error = None;
        for provider in providers {
            match provider.chat_stream(req.clone()).await {
                Ok(stream) => {
                    self.record_success(provider.id()).await;
                    return Ok(stream);
                }
                Err(e) => {
                    self.record_failure(provider.id()).await;
                    last_error = Some(e);
                    warn!(
                        "Provider {} stream failed: {:?}, trying next",
                        provider.id(),
                        last_error
                    );
                }
            }
        }
        Err(last_error.unwrap_or(LlmError::ProviderUnavailable("All providers failed".into())))
    }

    /// Récupère un provider par id (s'il est présent et pas en circuit open).
    pub async fn get_by_id(&self, provider_id: &str) -> Option<Arc<dyn LlmProvider>> {
        let providers = self.providers.read().await;
        let breakers = self.circuit_breakers.read().await;
        providers.iter().find(|p| p.id() == provider_id).cloned().filter(|p| {
            breakers
                .get(p.id())
                .is_none_or(|cb| cb.state != CircuitState::Open)
        })
    }

    /// Chat ciblé sur un provider précis (sélection explicite par l'UI).
    pub async fn chat_with(
        &self,
        provider_id: &str,
        req: ChatRequest,
    ) -> Result<ChatResponse, LlmError> {
        let provider = self
            .get_by_id(provider_id)
            .await
            .ok_or_else(|| LlmError::ProviderUnavailable(format!("Provider '{provider_id}' unavailable")))?;
        match provider.chat(req).await {
            Ok(resp) => {
                self.record_success(provider.id()).await;
                Ok(resp)
            }
            Err(e) => {
                self.record_failure(provider.id()).await;
                Err(e)
            }
        }
    }

    /// Chat stream ciblé sur un provider précis.
    pub async fn chat_stream_with(
        &self,
        provider_id: &str,
        req: ChatRequest,
    ) -> Result<TokenStream, LlmError> {
        let provider = self
            .get_by_id(provider_id)
            .await
            .ok_or_else(|| LlmError::ProviderUnavailable(format!("Provider '{provider_id}' unavailable")))?;
        match provider.chat_stream(req).await {
            Ok(stream) => {
                self.record_success(provider.id()).await;
                Ok(stream)
            }
            Err(e) => {
                self.record_failure(provider.id()).await;
                Err(e)
            }
        }
    }
}

pub struct RetryPolicy {
    max_retries: u32,
    base_delay: Duration,
    max_delay: Duration,
    backoff_multiplier: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            backoff_multiplier: 2.0,
        }
    }
}

impl RetryPolicy {
    pub async fn execute<F, Fut, T>(&self, mut f: F) -> Result<T, LlmError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, LlmError>>,
    {
        let mut delay = self.base_delay;
        for attempt in 0..=self.max_retries {
            match f().await {
                Ok(result) => return Ok(result),
                Err(e) if attempt < self.max_retries && self.is_retryable(&e) => {
                    warn!(
                        "Attempt {} failed: {:?}, retrying in {:?}",
                        attempt + 1,
                        e,
                        delay
                    );
                    tokio::time::sleep(delay).await;
                    delay = std::cmp::min(
                        Duration::from_millis(
                            (delay.as_millis() as f64 * self.backoff_multiplier) as u64,
                        ),
                        self.max_delay,
                    );
                }
                Err(e) => return Err(e),
            }
        }
        Err(LlmError::Internal("Max retries exceeded".into()))
    }

    fn is_retryable(&self, error: &LlmError) -> bool {
        matches!(
            error,
            LlmError::ProviderUnavailable(_)
                | LlmError::RateLimited(_)
                | LlmError::Timeout(_)
                | LlmError::ApiError(_)
        )
    }
}

#[async_trait]
pub trait LoadBalancer: Send + Sync {
    async fn select_provider(
        &self,
        providers: &[Arc<dyn LlmProvider>],
    ) -> Option<Arc<dyn LlmProvider>>;
}

pub struct RoundRobinBalancer {
    counter: Arc<Mutex<usize>>,
}

impl RoundRobinBalancer {
    pub fn new() -> Self {
        Self {
            counter: Arc::new(Mutex::new(0)),
        }
    }
}

#[async_trait]
impl LoadBalancer for RoundRobinBalancer {
    async fn select_provider(
        &self,
        providers: &[Arc<dyn LlmProvider>],
    ) -> Option<Arc<dyn LlmProvider>> {
        if providers.is_empty() {
            return None;
        }
        let mut counter = self.counter.lock().await;
        let idx = *counter % providers.len();
        *counter += 1;
        Some(providers[idx].clone())
    }
}

pub struct PriorityBalancer {
    priorities: std::collections::HashMap<String, u32>,
}

impl PriorityBalancer {
    pub fn new(priorities: std::collections::HashMap<String, u32>) -> Self {
        Self { priorities }
    }
}

#[async_trait]
impl LoadBalancer for PriorityBalancer {
    async fn select_provider(
        &self,
        providers: &[Arc<dyn LlmProvider>],
    ) -> Option<Arc<dyn LlmProvider>> {
        providers
            .iter()
            .max_by_key(|p| self.priorities.get(p.id()).copied().unwrap_or(0))
            .cloned()
    }
}
