use crate::commands::{
    agent_read, agents_list, chat_send, chat_stream, config_get, config_set, init_model_registry,
    init_plan_manager, init_provider_pool, init_provider_registry, init_session_manager,
    init_team_store, init_vault, llm_health_check, models_list, plan_cancel, plan_delete,
    plan_draft, plan_run, plan_save, plans_list, provider_test, providers_list, runtimes_detect,
    scan_local_models, session_cancel, session_create, session_delete, session_export, session_get,
    session_rename, session_send, sessions_list, team_delete, team_run, team_save, teams_list,
    vault_list, vault_path, vault_read, vault_write,
};
use crate::config::ConfigState;
use tauri::Manager;
use tracing::warn;

mod agents;
mod commands;
mod config;
mod discovery;
mod llm;
mod plan;
mod session;
mod store;
mod team;
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

            // Sessions : état hors coffre (app_data_dir), un fichier par session
            app.manage(init_session_manager(app));

            // Équipes : notion du harness, assemblées depuis les agents du coffre
            app.manage(init_team_store(app));

            // Plans : étapes assignées, exécutées en parallèle quand elles le peuvent
            app.manage(init_plan_manager(app));

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
            // Runtimes locaux
            runtimes_detect,
            // Sessions
            sessions_list,
            session_create,
            session_get,
            session_rename,
            session_delete,
            session_send,
            session_cancel,
            session_export,
            // Équipes
            teams_list,
            team_save,
            team_delete,
            team_run,
            // Plans
            plans_list,
            plan_save,
            plan_delete,
            plan_draft,
            plan_run,
            plan_cancel,
            // Config
            config_get,
            config_set,
            // Coffre
            vault_list,
            vault_read,
            vault_write,
            vault_path,
            // Agents
            agents_list,
            agent_read,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
