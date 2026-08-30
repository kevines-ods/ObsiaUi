/**
 * Wrappers typés autour de `@tauri-apps/api` — alignés sur le contrat Rust.
 *
 * - `invoke(...)` : les commandes Tauri reçoivent leurs arguments **camelCase**
 *   (conversion `snake_case` → `camelCase` faite par Tauri pour les noms
 *   d'arguments au niveau racine, ex. `provider_id` → `providerId`).
 * - `listen(...)` : événements de stream `llm:*`, payloads typés.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

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
  RuntimeScan,
  VaultEntry,
} from "../types/ipc";

/** Commande non-streaming (réponse complète). */
export const chatSend = (payload: ChatRequestPayload): Promise<ChatResponse> =>
  invoke<ChatResponse>("chat_send", { payload });

/** Commande streaming : émet `llm:token`, `llm:done`, `llm:error`. */
export const chatStream = (payload: ChatRequestPayload): Promise<void> =>
  invoke<void>("chat_stream", { payload });

export const providersList = (): Promise<ProviderInfo[]> =>
  invoke<ProviderInfo[]>("providers_list");

export const providerTest = (providerId: string): Promise<ProviderHealth> =>
  invoke<ProviderHealth>("provider_test", { providerId });

export const modelsList = (providerId?: string | null): Promise<ModelInfo[]> =>
  invoke<ModelInfo[]>("models_list", { providerId: providerId ?? undefined });

export const llmHealthCheck = (): Promise<ProviderHealth[]> =>
  invoke<ProviderHealth[]>("llm_health_check");

export const scanLocalModels = (): Promise<ModelInfo[]> =>
  invoke<ModelInfo[]>("scan_local_models");

/**
 * Détecte les moteurs LLM locaux (Ollama, llama.cpp) et recâble les providers
 * sur les adresses qui répondent réellement.
 */
export const runtimesDetect = (): Promise<RuntimeScan> =>
  invoke<RuntimeScan>("runtimes_detect");

export const configGet = (): Promise<ConfigView> =>
  invoke<ConfigView>("config_get");

export const configSet = (patch: ConfigPatch): Promise<ConfigView> =>
  invoke<ConfigView>("config_set", { patch });

export const vaultList = (): Promise<VaultEntry[]> =>
  invoke<VaultEntry[]>("vault_list");

export const vaultRead = (relPath: string): Promise<string> =>
  invoke<string>("vault_read", { relPath });

export const vaultWrite = (
  relPath: string,
  content: string,
): Promise<VaultEntry> => invoke<VaultEntry>("vault_write", { relPath, content });

export const vaultPath = (): Promise<string> =>
  invoke<string>("vault_path");

/** Agents : liste validée par le backend (frontmatter parsé côté Rust). */
export const agentsList = (): Promise<AgentInfo[]> =>
  invoke<AgentInfo[]>("agents_list");

/** Lit un agent complet (chemin relatif `IA/agents/*.md` ou simple nom). */
export const agentRead = (path: string): Promise<AgentDoc> =>
  invoke<AgentDoc>("agent_read", { path });

/** Écoute un événement de stream et retourne une fonction de désabonnement. */
export function listenEvent<T>(
  event: LlmStreamEventName,
  handler: (payload: T) => void,
): Promise<UnlistenFn> {
  return listen<T>(event, (e) => handler(e.payload));
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
