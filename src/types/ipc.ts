/**
 * Contrat IPC Tauri — miroir TypeScript des types Serde du backend
 * (`src-tauri/src/`).
 *
 * ⚠️ Convention de nommage EXACTE (elle diffère selon le module Rust) :
 *  - `llm/provider.rs` (ChatMessage, ChatResponse, ChatChoice, Usage,
 *    ModelInfo, ModelPricing) : PAS de `rename_all` → champs **snake_case**.
 *  - `commands.rs` / `config.rs` / `vault.rs` (ChatRequestPayload, ProviderInfo,
 *    ProviderHealth, ConfigView, ConfigPatch, SetApiKey, VaultEntry) :
 *    `#[serde(rename_all = "camelCase")]` → champs **camelCase**.
 *
 * Ne pas « corriger » ces noms en camelCase global : ils suivent la
 * sérialisation réelle du Rust.
 */

// ===== Chat (provider.rs = snake_case) =====

export type ChatRole = "system" | "user" | "assistant";

export interface ChatMessage {
  role: ChatRole;
  content: string;
  name?: string | null;
}

export interface Usage {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
}

export interface ChatChoice {
  index: number;
  message: ChatMessage;
  finish_reason?: string | null;
}

export interface ChatResponse {
  id: string;
  model: string;
  choices: ChatChoice[];
  usage?: Usage | null;
}

// ===== Modèles (provider.rs = snake_case) =====

export type ModelCapability =
  | "Chat"
  | "Completion"
  | "Embedding"
  | "Vision"
  | "Audio"
  | "ToolUse"
  | "Reasoning";

export interface ModelPricing {
  input_per_1k: number;
  output_per_1k: number;
  currency: string;
}

export interface ModelInfo {
  id: string;
  name: string;
  provider: string;
  context_window: number;
  capabilities: ModelCapability[];
  pricing?: ModelPricing | null;
  local_path?: string | null;
}

// ===== Providers (commands.rs = camelCase) =====

export interface ProviderInfo {
  id: string;
  name: string;
  models: ModelInfo[];
}

export interface ProviderHealth {
  providerId: string;
  ok: boolean;
  error?: string | null;
}

// ===== Chat payload (commands.rs = camelCase) =====

export interface ChatRequestPayload {
  provider?: string | null;
  model: string;
  messages: ChatMessage[];
  temperature?: number | null;
  maxTokens?: number | null;
}

// ===== Config (config.rs = camelCase) =====

export interface SetApiKey {
  providerId: string;
  apiKey: string;
}

export interface ConfigPatch {
  setApiKey?: SetApiKey | null;
  vaultPath?: string | null;
  defaultProvider?: string | null;
  ollamaHost?: string | null;
}

export interface ConfigView {
  apiKeysConfigured: string[];
  vaultPath?: string | null;
  defaultProvider?: string | null;
  ollamaHost?: string | null;
}

// ===== Coffre (vault.rs = camelCase) =====

export interface VaultEntry {
  path: string;
  name: string;
  modified: string;
}

// ===== Événements de stream =====

export type LlmTokenEvent = string;
export type LlmDoneEvent = ChatResponse;
export type LlmErrorEvent = string;

export type LlmStreamEventName = "llm:token" | "llm:done" | "llm:error";
