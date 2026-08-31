/**
 * Wrappers typés autour de `@tauri-apps/api` — alignés sur le contrat Rust.
 *
 * Tout passe par `lib/transport` : les mêmes appels atteignent le harness
 * local ou une instance distante, sans que ce module ait à le savoir.
 *
 * - Les arguments sont en **camelCase** (conversion `snake_case` →
 *   `camelCase` faite par Tauri pour les noms d'arguments au niveau racine,
 *   ex. `provider_id` → `providerId`). Le serveur distant attend exactement
 *   la même forme.
 * - Les abonnements renvoient une fonction de désabonnement.
 */
import { call, callLocal, subscribe } from "./transport";

import type {
  AgentDoc,
  AgentInfo,
  ChatRequestPayload,
  ChatResponse,
  ConfigPatch,
  ConfigView,
  LlmDoneEvent,
  LlmErrorEvent,
  LlmStreamEventName,
  LlmTokenEvent,
  ModelInfo,
  ProviderHealth,
  ProviderInfo,
  Plan,
  PlanDraftPayload,
  PlanSavePayload,
  PlanTokenEvent,
  PlanUpdateEvent,
  InstalledPlugin,
  LoadedPlugin,
  ActionResult,
  IntendantAction,
  McpInfo,
  PatchSavePayload,
  Proposition,
  VaultGraph,
  RemoteStatus,
  UiPatch,
  RuntimeScan,
  Session,
  SessionCreatePayload,
  SessionDoneEvent,
  SessionErrorEvent,
  SessionMeta,
  SessionMessageEvent,
  SessionTokenEvent,
  SessionTurnEvent,
  Team,
  TeamSavePayload,
  VaultEntry,
} from "../types/ipc";

/** Commande non-streaming (réponse complète). */
export const chatSend = (payload: ChatRequestPayload): Promise<ChatResponse> =>
  call<ChatResponse>("chat_send", { payload });

/** Commande streaming : émet `llm:token`, `llm:done`, `llm:error`. */
export const chatStream = (payload: ChatRequestPayload): Promise<void> =>
  call<void>("chat_stream", { payload });

export const providersList = (): Promise<ProviderInfo[]> =>
  call<ProviderInfo[]>("providers_list");

export const providerTest = (providerId: string): Promise<ProviderHealth> =>
  call<ProviderHealth>("provider_test", { providerId });

export const modelsList = (providerId?: string | null): Promise<ModelInfo[]> =>
  call<ModelInfo[]>("models_list", { providerId: providerId ?? undefined });

export const llmHealthCheck = (): Promise<ProviderHealth[]> =>
  call<ProviderHealth[]>("llm_health_check");

export const scanLocalModels = (): Promise<ModelInfo[]> =>
  call<ModelInfo[]>("scan_local_models");

/**
 * Détecte les moteurs LLM locaux (Ollama, llama.cpp) et recâble les providers
 * sur les adresses qui répondent réellement.
 */
export const runtimesDetect = (): Promise<RuntimeScan> =>
  call<RuntimeScan>("runtimes_detect");

export const configGet = (): Promise<ConfigView> =>
  call<ConfigView>("config_get");

export const configSet = (patch: ConfigPatch): Promise<ConfigView> =>
  call<ConfigView>("config_set", { patch });

export const vaultList = (): Promise<VaultEntry[]> =>
  call<VaultEntry[]>("vault_list");

export const vaultRead = (relPath: string): Promise<string> =>
  call<string>("vault_read", { relPath });

export const vaultWrite = (
  relPath: string,
  content: string,
): Promise<VaultEntry> => call<VaultEntry>("vault_write", { relPath, content });

export const vaultPath = (): Promise<string> =>
  call<string>("vault_path");

/** Agents : liste validée par le backend (frontmatter parsé côté Rust). */
export const agentsList = (): Promise<AgentInfo[]> =>
  call<AgentInfo[]>("agents_list");

/** Lit un agent complet (chemin relatif `IA/agents/*.md` ou simple nom). */
export const agentRead = (path: string): Promise<AgentDoc> =>
  call<AgentDoc>("agent_read", { path });

// ===== Sessions =====

export const sessionsList = (): Promise<SessionMeta[]> =>
  call<SessionMeta[]>("sessions_list");

export const sessionCreate = (payload: SessionCreatePayload): Promise<SessionMeta> =>
  call<SessionMeta>("session_create", { payload });

export const sessionGet = (sessionId: string): Promise<Session> =>
  call<Session>("session_get", { sessionId });

export const sessionRename = (sessionId: string, title: string): Promise<SessionMeta> =>
  call<SessionMeta>("session_rename", { sessionId, title });

export const sessionDelete = (sessionId: string): Promise<void> =>
  call<void>("session_delete", { sessionId });

/** Envoie un message ; la réponse arrive via les événements `session:*`. */
export const sessionSend = (sessionId: string, content: string): Promise<void> =>
  call<void>("session_send", { sessionId, content });

/** Interrompt le tour en cours ; le texte déjà produit est conservé. */
export const sessionCancel = (sessionId: string): Promise<boolean> =>
  call<boolean>("session_cancel", { sessionId });

/** Exporte la session en note Markdown dans `brouillon/` du coffre. */
export const sessionExport = (
  sessionId: string,
  project: string,
): Promise<VaultEntry> =>
  call<VaultEntry>("session_export", { sessionId, project });

/**
 * Abonnements aux événements de session. Un seul abonnement couvre toutes les
 * sessions ouvertes : la charge utile porte `sessionId`.
 */
export const sessionStream = {
  onToken: (handler: (e: SessionTokenEvent) => void) =>
    subscribe<SessionTokenEvent>("session:token", handler),
  /** Un autre membre de l'équipe prend la parole. */
  onTurn: (handler: (e: SessionTurnEvent) => void) =>
    subscribe<SessionTurnEvent>("session:turn", handler),
  /** Une intervention vient d'être ajoutée au fil. */
  onMessage: (handler: (e: SessionMessageEvent) => void) =>
    subscribe<SessionMessageEvent>("session:message", handler),
  onDone: (handler: (e: SessionDoneEvent) => void) =>
    subscribe<SessionDoneEvent>("session:done", handler),
  onError: (handler: (e: SessionErrorEvent) => void) =>
    subscribe<SessionErrorEvent>("session:error", handler),
};

// ===== Équipes =====

export const teamsList = (): Promise<Team[]> => call<Team[]>("teams_list");

export const teamSave = (payload: TeamSavePayload): Promise<Team> =>
  call<Team>("team_save", { payload });

export const teamDelete = (teamId: string): Promise<void> =>
  call<void>("team_delete", { teamId });

/** Lance l'équipe de la session sur un objectif ; réponses via `session:*`. */
export const teamRun = (sessionId: string, objective: string): Promise<void> =>
  call<void>("team_run", { sessionId, objective });

// ===== Intendant =====

/** Prompt système de l'intendant, tel qu'il sera envoyé. */
export const intendantPrompt = (): Promise<string> => call<string>("intendant_prompt");

/**
 * Envoie un message à l'intendant. Rien n'est appliqué : les actions
 * proposées reviennent pour validation.
 */
export const intendantSend = (
  sessionId: string,
  content: string,
): Promise<Proposition | null> =>
  call<Proposition | null>("intendant_send", { sessionId, content });

/** Applique les actions validées ; le résultat est rendu action par action. */
export const intendantApply = (actions: IntendantAction[]): Promise<ActionResult[]> =>
  call<ActionResult[]>("intendant_apply", { actions });

// ===== Graphe du coffre =====

/** Notes, liens résolus, liens cassés et étiquettes du coffre. */
export const vaultGraph = (): Promise<VaultGraph> => call<VaultGraph>("vault_graph");

/** Ouvre une note dans Obsidian ; renvoie l'URI utilisée. */
export const vaultOpenExternal = (relPath: string): Promise<string> =>
  call<string>("vault_open_external", { relPath });

// ===== Outils MCP =====

/** Outils déclarés dans le coffre, avec les agents qui les utilisent. */
export const mcpList = (): Promise<McpInfo[]> => call<McpInfo[]>("mcp_list");

/** Rédige une déclaration dans `brouillon/` ; renvoie son chemin. */
export const mcpDraft = (
  name: string,
  description: string,
  body: string,
): Promise<string> => call<string>("mcp_draft", { name, description, body });

// ===== Patches d'interface et plugins =====
//
// Toujours en local : ils modifient CETTE fenêtre. Un hôte distant fournit le
// travail, pas l'apparence du poste client.

export const patchesList = (): Promise<UiPatch[]> =>
  callLocal<UiPatch[]>("patches_list");

export const patchSave = (payload: PatchSavePayload): Promise<UiPatch> =>
  callLocal<UiPatch>("patch_save", { payload });

export const patchDelete = (patchId: string): Promise<void> =>
  callLocal<void>("patch_delete", { patchId });

/** Active ou désactive un patch ; renvoie le CSS cumulé qui en résulte. */
export const patchToggle = (patchId: string, enabled: boolean): Promise<string> =>
  callLocal<string>("patch_toggle", { patchId, enabled });

/** CSS cumulé des patches actifs, à poser sur `:root`. */
export const patchCss = (): Promise<string> => callLocal<string>("patch_css");

export const pluginsList = (): Promise<InstalledPlugin[]> =>
  callLocal<InstalledPlugin[]>("plugins_list");

/** Plugins actifs, avec leur code et les commandes qui leur sont ouvertes. */
export const pluginsLoad = (): Promise<LoadedPlugin[]> =>
  callLocal<LoadedPlugin[]>("plugins_load");

export const pluginsDir = (): Promise<string> => callLocal<string>("plugins_dir");

/** Active un plugin en approuvant le code présent sur disque. */
export const pluginEnable = (pluginId: string): Promise<InstalledPlugin> =>
  callLocal<InstalledPlugin>("plugin_enable", { pluginId });

export const pluginDisable = (pluginId: string): Promise<void> =>
  callLocal<void>("plugin_disable", { pluginId });

// ===== Serveur distant =====
//
// Toujours en local : ces commandes portent sur CETTE machine, et le serveur
// ne les expose pas — un client attaché ne reconfigure pas son hôte.

export const remoteStatus = (): Promise<RemoteStatus> =>
  callLocal<RemoteStatus>("remote_status");

export const remoteStart = (): Promise<RemoteStatus> =>
  callLocal<RemoteStatus>("remote_start");

export const remoteStop = (): Promise<RemoteStatus> =>
  callLocal<RemoteStatus>("remote_stop");

/** Révèle le jeton, pour le recopier sur le poste client. */
export const remoteTokenRead = (): Promise<string> =>
  callLocal<string>("remote_token_read");

/** Engendre un nouveau jeton et invalide l'ancien. */
export const remoteTokenRotate = (): Promise<string> =>
  callLocal<string>("remote_token_rotate");

// ===== Plans =====

export const plansList = (): Promise<Plan[]> => call<Plan[]>("plans_list");

export const planSave = (payload: PlanSavePayload): Promise<Plan> =>
  call<Plan>("plan_save", { payload });

export const planDelete = (planId: string): Promise<void> =>
  call<void>("plan_delete", { planId });

/** Fait décomposer un objectif par un modèle. Le plan n'est pas enregistré. */
export const planDraft = (payload: PlanDraftPayload): Promise<Plan> =>
  call<Plan>("plan_draft", { payload });

/** Exécute le plan ; l'avancement arrive via `plan:update`. */
export const planRun = (planId: string): Promise<Plan> =>
  call<Plan>("plan_run", { planId });

export const planCancel = (planId: string): Promise<boolean> =>
  call<boolean>("plan_cancel", { planId });

export const planStream = {
  /** Fragment produit par une étape en cours. */
  onToken: (handler: (e: PlanTokenEvent) => void) =>
    subscribe<PlanTokenEvent>("plan:token", handler),
  /** Nouvel état du plan après chaque vague d'étapes. */
  onUpdate: (handler: (e: PlanUpdateEvent) => void) =>
    subscribe<PlanUpdateEvent>("plan:update", handler),
};

/** Écoute un événement de stream et retourne une fonction de désabonnement. */
export function listenEvent<T>(
  event: LlmStreamEventName,
  handler: (payload: T) => void,
): Promise<() => void> {
  return subscribe<T>(event, handler);
}

/** Abonnements typés aux trois événements de streaming LLM. */
export const stream = {
  onToken: (handler: (payload: LlmTokenEvent) => void) =>
    listenEvent<LlmTokenEvent>("llm:token", handler),
  onDone: (handler: (payload: LlmDoneEvent) => void) =>
    listenEvent<LlmDoneEvent>("llm:done", handler),
  onError: (handler: (payload: LlmErrorEvent) => void) =>
    listenEvent<LlmErrorEvent>("llm:error", handler),
};
