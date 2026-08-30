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
  llamacppHost?: string | null;
}

export interface ConfigView {
  apiKeysConfigured: string[];
  vaultPath?: string | null;
  defaultProvider?: string | null;
  ollamaHost?: string | null;
  llamacppHost?: string | null;
}

// ===== Runtimes locaux (discovery.rs) =====

/** `RuntimeKind` est sérialisé en kebab-case côté Rust. */
export type RuntimeKind = "ollama" | "llama-cpp";

/**
 * Origine d'une adresse détectée — énum Rust étiquetée
 * (`tag = "type"`, `content = "detail"`), variantes en camelCase.
 */
export type RuntimeOrigin =
  | { type: "config" }
  | { type: "env"; detail: string }
  | { type: "process"; detail: string }
  | { type: "defaultPort" };

export interface DetectedRuntime {
  kind: RuntimeKind;
  /** Identifiant du provider correspondant (`ollama`, `llamacpp`). */
  providerId: string;
  label: string;
  baseUrl: string;
  origin: RuntimeOrigin;
  reachable: boolean;
  version?: string | null;
  models: string[];
  error?: string | null;
}

export interface DetectedBinary {
  kind: RuntimeKind;
  name: string;
  path: string;
}

export interface RuntimeScan {
  /** Adresses sondées, joignables en premier. */
  runtimes: DetectedRuntime[];
  /** Binaires présents dans le PATH, daemon démarré ou non. */
  binaries: DetectedBinary[];
}

// ===== Sessions (session.rs = camelCase) =====

export interface SessionMessage {
  role: ChatRole;
  content: string;
  /** Horodatage Unix en secondes. */
  at: number;
  /** Agent auteur — renseigné pour les sessions d'équipe. */
  agent?: string | null;
}

export interface SessionMeta {
  id: string;
  title: string;
  agent?: string | null;
  /** Équipe pilotant la session, exclusive de `agent`. */
  team?: string | null;
  provider?: string | null;
  model: string;
  createdAt: number;
  updatedAt: number;
  messageCount: number;
}

/** Session complète : `SessionMeta` aplati + historique. */
export interface Session extends SessionMeta {
  messages: SessionMessage[];
}

export interface SessionCreatePayload {
  agent?: string | null;
  team?: string | null;
  provider?: string | null;
  model: string;
}

// ===== Équipes (team.rs = camelCase) =====

export interface TeamMember {
  /** Nom de l'agent du coffre. */
  agent: string;
  provider?: string | null;
  model: string;
  /** Consigne propre à ce membre dans cette équipe. */
  role?: string | null;
}

/** Sérialisé en kebab-case côté Rust. */
export type TeamStrategy = "round-robin" | "supervisor";

export interface Team {
  id: string;
  name: string;
  description: string;
  members: TeamMember[];
  strategy: TeamStrategy;
  /** Garde-fou : nombre maximal de tours de parole par exécution. */
  maxTurns: number;
  createdAt: number;
  updatedAt: number;
}

export interface TeamSavePayload {
  /** Absent = création. */
  id?: string | null;
  name: string;
  description: string;
  members: TeamMember[];
  strategy: TeamStrategy;
  maxTurns: number;
}

// ===== Événements de session =====

export interface SessionTokenEvent {
  sessionId: string;
  token: string;
}

export interface SessionDoneEvent {
  sessionId: string;
  message: SessionMessage;
  meta: SessionMeta;
  /** Vrai si le tour a été interrompu par `session_cancel`. */
  cancelled: boolean;
}

export interface SessionErrorEvent {
  sessionId: string;
  error: string;
}

/** Message ajouté au fil pendant une exécution (tours d'équipe). */
export interface SessionMessageEvent {
  sessionId: string;
  message: SessionMessage;
  meta: SessionMeta;
}

/** Changement d'orateur dans une session d'équipe. */
export interface SessionTurnEvent {
  sessionId: string;
  agent: string;
  turn: number;
}

// ===== Coffre (vault.rs = camelCase) =====

export interface VaultEntry {
  path: string;
  name: string;
  modified: string;
}

// ===== Agents (agents.rs = camelCase) =====

export interface AgentInfo {
  /** Chemin relatif au coffre, ex. `IA/agents/assistant.md`. */
  path: string;
  name: string;
  description: string;
  /** Toujours un tableau (défaut `[]` côté backend). */
  skills: string[];
  /** Idem. */
  mcp: string[];
  /** Bool Rust (plus besoin de parser "true"/"false"). */
  readOnly: boolean;
}

export interface AgentDoc extends AgentInfo {
  /** Corps markdown de l'agent, SANS le frontmatter YAML (prompt système). */
  content: string;
}

// ===== Événements de stream =====

export type LlmTokenEvent = string;
export type LlmDoneEvent = ChatResponse;
export type LlmErrorEvent = string;

export type LlmStreamEventName = "llm:token" | "llm:done" | "llm:error";
