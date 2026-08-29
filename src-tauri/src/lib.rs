use crate::commands::{
    chat_send, chat_stream, config_get, config_set, init_model_registry, init_provider_pool,
    init_provider_registry, init_vault, llm_health_check, models_list, provider_test,
    providers_list, scan_local_models, vault_list, vault_path, vault_read, vault_write,
};
use crate::config::ConfigState;
use tauri::Manager;
use tracing::warn;

mod commands;
mod config;
mod llm;
mod vault;

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Config app (clés API hors repo, fichier 0600 dans app_config_dir)
            let config_dir = app
                .path()
                .app_config_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."));
            let config_path = config_dir.join("obsia_config.json");
            let config_state = ConfigState::new(config_path);
            app.manage(config_state);

            // Registry + pool : le pool est synchronisé depuis le registry
            // (les providers API sont enregistrés si une clé est dispo).
            let config_ref = app.state::<ConfigState>().inner();
            let provider_registry = init_provider_registry(config_ref);
            let model_registry = init_model_registry();
            let provider_pool = init_provider_pool(&provider_registry);

            // Coffre : sandbox bornée à obsia_vault/ (config > env > défaut dev)
            match init_vault(config_ref) {
                Ok(vault) => {
                    app.manage(vault);
                }
                Err(e) => {
                    warn!(%e, "coffre indisponible au démarrage");
                    app.manage(crate::vault::VaultState::unavailable(e));
                }
            }

            app.manage(provider_registry);
            app.manage(model_registry);
            app.manage(provider_pool);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Chat
            chat_send,
            chat_stream,
            // Providers
            providers_list,
            provider_test,
            models_list,
            llm_health_check,
            scan_local_models,
            // Config
            config_get,
            config_set,
            // Coffre
            vault_list,
            vault_read,
            vault_write,
            vault_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
