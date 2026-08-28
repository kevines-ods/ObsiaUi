use tauri::{Emitter, Manager};
use crate::llm::{OllamaProvider, OpenAIProvider, AnthropicProvider, OpenRouterProvider, GeminiProvider};
use crate::commands::{
    list_providers, chat_stream, chat, llm_list_models, llm_health_check,
    scan_local_models, list_vault, get_backlinks,
    init_provider_registry, init_model_registry, init_provider_pool,
};

mod llm;
mod commands;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let provider_registry = init_provider_registry();
            let model_registry = init_model_registry();
            let provider_pool = init_provider_pool(provider_registry.clone());

            app.manage(provider_registry);
            app.manage(model_registry);
            app.manage(provider_pool);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            list_providers,
            chat_stream,
            chat,
            llm_list_models,
            llm_health_check,
            scan_local_models,
            list_vault,
            get_backlinks,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}