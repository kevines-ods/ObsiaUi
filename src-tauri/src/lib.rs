use crate::commands::{
    agent_read, agents_list, chat_send, chat_stream, config_get, config_set, init_model_registry,
    init_plan_manager, init_plugin_store, init_provider_pool, init_provider_registry,
    init_session_manager, init_team_store, init_vault, intendant_apply, intendant_prompt,
    intendant_send, llm_health_check, mcp_draft, mcp_list, models_list, patch_css, patch_delete,
    patch_save, patch_toggle, patches_list, plan_cancel, plan_delete, plan_draft, plan_run,
    plan_save, plans_list, plugin_disable, plugin_enable, plugins_dir, plugins_list, plugins_load,
    provider_test, providers_list, remote_start, remote_status, remote_stop, remote_token_read,
    remote_token_rotate, runtimes_detect, scan_local_models, session_cancel, session_create,
    session_delete, session_export, session_get, session_rename, session_send, sessions_list,
    team_delete, team_run, team_save, teams_list, vault_graph, vault_list, vault_open_external,
    vault_path, vault_read, vault_write,
};
use crate::config::ConfigState;
use tauri::Manager;
use tracing::warn;

mod agents;
mod commands;
mod config;
mod discovery;
mod event;
mod graph;
mod intendant;
mod llm;
mod mcp;
mod plan;
mod plugin;
mod remote;
mod session;
mod store;
mod team;
mod vault;

/// Réémet vers la fenêtre les événements du bus.
///
/// Le cœur n'écrit plus dans le webview : il publie sur le bus, et ce pont
/// est l'un de ses abonnés. Un client distant en sera un autre, recevant
/// exactement le même flux.
fn pont_evenements_tauri(app: tauri::AppHandle, bus: std::sync::Arc<crate::event::EventBus>) {
    use tauri::Emitter;
    tauri::async_runtime::spawn(async move {
        let mut rx = bus.subscribe();
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let _ = app.emit(&ev.name, ev.payload);
                }
                // La fenêtre a pris du retard : on reprend au plus ancien
                // disponible plutôt que d'abandonner le pont.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(perdus)) => {
                    warn!(perdus, "fenêtre en retard sur le bus d'événements");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

/// Redémarre le serveur distant au lancement s'il était activé.
///
/// Fait après le `setup`, en tâche de fond : l'ouverture d'une socket ne doit
/// pas retarder l'affichage de la fenêtre, et un port déjà pris ne doit pas
/// empêcher l'application de démarrer.
fn demarrer_serveur_distant_si_active(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let config = app.state::<std::sync::Arc<ConfigState>>();
        let cfg = config.read();
        if !cfg.remote_enabled {
            return;
        }
        let Some(jeton) = cfg.remote_token() else {
            warn!("serveur distant activé sans jeton — démarrage ignoré");
            return;
        };
        let bind = cfg
            .remote_bind
            .clone()
            .filter(|b| !b.trim().is_empty())
            .unwrap_or_else(|| format!("127.0.0.1:{}", crate::remote::PORT_DEFAUT));
        let harness = crate::remote::Harness {
            bus: app
                .state::<std::sync::Arc<crate::event::EventBus>>()
                .inner()
                .clone(),
            sessions: app
                .state::<crate::commands::SessionManagerState>()
                .inner()
                .clone(),
            teams: app
                .state::<crate::commands::TeamStoreState>()
                .inner()
                .clone(),
            plans: app
                .state::<crate::commands::PlanManagerState>()
                .inner()
                .clone(),
            pool: app
                .state::<crate::commands::ProviderPoolState>()
                .inner()
                .clone(),
            registry: app
                .state::<crate::commands::ProviderRegistryState>()
                .inner()
                .clone(),
            vault: app
                .state::<crate::commands::VaultStateArc>()
                .inner()
                .clone(),
        };
        let etat = app.state::<std::sync::Arc<crate::remote::RemoteState>>();
        if let Err(e) = etat.demarrer(harness, &bind, &jeton).await {
            warn!(%e, "serveur distant non démarré");
        }
    });
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Config app (clés API hors repo, fichier 0600 dans app_config_dir)
            let config_dir = app
                .path()
                .app_config_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."));
            let config_path = config_dir.join("obsia_config.json");
            let config_state = std::sync::Arc::new(ConfigState::new(config_path));
            app.manage(config_state);

            // Bus d'événements : source unique, consommée par la fenêtre
            // locale et — plus tard — par les clients distants.
            let bus = std::sync::Arc::new(crate::event::EventBus::new());
            app.manage(bus.clone());
            pont_evenements_tauri(app.handle().clone(), bus);

            // Registry + pool : le pool est synchronisé depuis le registry
            // (les providers API sont enregistrés si une clé est dispo).
            let config_ref: &ConfigState = &app.state::<std::sync::Arc<ConfigState>>();
            let provider_registry = init_provider_registry(config_ref);
            let model_registry = init_model_registry();
            let provider_pool = init_provider_pool(&provider_registry);

            // Coffre : sandbox bornée à obsia_vault/ (config > env > défaut dev)
            match init_vault(config_ref) {
                Ok(vault) => {
                    app.manage(std::sync::Arc::new(vault));
                }
                Err(e) => {
                    warn!(%e, "coffre indisponible au démarrage");
                    app.manage(std::sync::Arc::new(crate::vault::VaultState::unavailable(
                        e,
                    )));
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

            // Serveur distant : arrêté par défaut, démarré à la demande ou
            // au lancement si l'utilisateur l'a activé.
            app.manage(std::sync::Arc::new(crate::remote::RemoteState::new()));
            demarrer_serveur_distant_si_active(app.handle().clone());

            // Patches d'interface et plugins : inertes tant qu'ils ne sont
            // pas explicitement activés.
            app.manage(init_plugin_store(app));

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
            // Serveur distant
            remote_status,
            remote_start,
            remote_stop,
            remote_token_read,
            remote_token_rotate,
            // Interface et plugins
            patches_list,
            patch_save,
            patch_delete,
            patch_toggle,
            patch_css,
            plugins_list,
            plugins_load,
            plugins_dir,
            plugin_enable,
            plugin_disable,
            // MCP
            mcp_list,
            mcp_draft,
            // Intendant
            intendant_prompt,
            intendant_send,
            intendant_apply,
            // Graphe du coffre
            vault_graph,
            vault_open_external,
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
