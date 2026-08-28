pub mod anthropic;
pub mod fallback;
pub mod gemini;
pub mod ollama;
pub mod openai;
pub mod openrouter;
pub mod provider;
pub mod registry;
pub mod streaming;

// Surface API publique du module llm : consommée par les futurs crates /
// plugins externes, pas par la crate binaire elle-même (d'où l'allow).
#[allow(unused_imports)]
pub use anthropic::AnthropicProvider;
#[allow(unused_imports)]
pub use fallback::{PoolStrategy, ProviderPool};
#[allow(unused_imports)]
pub use gemini::GeminiProvider;
#[allow(unused_imports)]
pub use ollama::OllamaProvider;
#[allow(unused_imports)]
pub use openai::OpenAIProvider;
#[allow(unused_imports)]
pub use openrouter::OpenRouterProvider;
#[allow(unused_imports)]
pub use provider::{ChatRequest, ChatResponse, ModelInfo};
#[allow(unused_imports)]
pub use registry::{ModelRegistry, ProviderRegistry};
#[allow(unused_imports)]
pub use streaming::StreamingManager;
