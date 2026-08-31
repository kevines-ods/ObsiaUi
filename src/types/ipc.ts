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

export type Theme = "dark" | "light" | "system";

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
  theme?: Theme | null;
  remoteEnabled?: boolean | null;
  remoteBind?: string | null;
}

export interface ConfigView {
  apiKeysConfigured: string[];
  vaultPath?: string | null;
  defaultProvider?: string | null;
  ollamaHost?: string | null;
  llamacppHost?: string | null;
  /** `dark` (défaut), `light` ou `system`. */
  theme: Theme;
  remoteEnabled: boolean;
  remoteBind?: string | null;
  /** Présence du jeton seulement ; sa valeur se lit par `remoteTokenRead`. */
  remoteTokenConfigured: boolean;
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

// ===== Plans (plan.rs = camelCase) =====

/** Sérialisés en kebab-case côté Rust. */
export type StepStatus = "pending" | "running" | "done" | "failed" | "skipped";
export type PlanStatus = "draft" | "running" | "done" | "incomplete" | "cancelled";

export interface PlanStep {
  /** Identifiant court, unique dans le plan ; cible des dépendances. */
  id: string;
  title: string;
  instruction: string;
  agent: string;
  provider?: string | null;
  model: string;
  dependsOn: string[];
  status: StepStatus;
  result?: string | null;
  error?: string | null;
  startedAt?: number | null;
  finishedAt?: number | null;
}

export interface Plan {
  id: string;
  title: string;
  objective: string;
  steps: PlanStep[];
  status: PlanStatus;
  createdAt: number;
  updatedAt: number;
}

export interface PlanSavePayload {
  id?: string | null;
  title: string;
  objective: string;
  steps: PlanStep[];
}

export interface PlanDraftPayload {
  objective: string;
  /** Agent affecté aux étapes dont l'agent proposé est inconnu. */
  agent: string;
  provider?: string | null;
  model: string;
}

export interface PlanTokenEvent {
  planId: string;
  stepId: string;
  token: string;
}

export interface PlanUpdateEvent {
  plan: Plan;
}

// ===== Interface et plugins (plugin.rs = camelCase) =====

export interface LayoutPatch {
  leftOpen?: boolean | null;
  rightOpen?: boolean | null;
  /** Largeur du panneau de contrôle, en pixels. */
  leftWidth?: number | null;
  /** Largeur du gestionnaire de fichiers, en pixels. */
  rightWidth?: number | null;
  /** Ordre des sections du panneau de contrôle. */
  panels?: string[] | null;
}

export interface UiPatch {
  id: string;
  name: string;
  description: string;
  /** Jetons de thème : nom (sans `--`) vers valeur CSS validée. */
  theme: Record<string, string>;
  layout: LayoutPatch;
  enabled: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface PatchSavePayload {
  id?: string | null;
  name: string;
  description: string;
  theme: Record<string, string>;
  layout: LayoutPatch;
}

/** Sérialisés en kebab-case côté Rust. */
export type MountPoint = "control-panel" | "chat-toolbar" | "status-bar";
export type PluginPermission = "vault-read" | "vault-write" | "sessions" | "providers";

export interface PluginManifest {
  id: string;
  name: string;
  version: string;
  description: string;
  entry: string;
  mount: MountPoint[];
  permissions: PluginPermission[];
}

/** Manifeste aplati + état d'activation. */
export interface InstalledPlugin extends PluginManifest {
  enabled: boolean;
  approvedDigest?: string | null;
  currentDigest?: string | null;
  /** Vrai quand le fichier a changé depuis l'activation. */
  needsReview: boolean;
}

export interface LoadedPlugin extends InstalledPlugin {
  source: string;
  /** Commandes que ses permissions lui ouvrent. */
  allowedCommands: string[];
}

// ===== Outils MCP (mcp.rs = camelCase) =====

export interface McpInfo {
  /** Chemin relatif au coffre, ex. `IA/MCP/git-hub.md`. */
  path: string;
  name: string;
  description: string;
  /** Agents qui le déclarent dans leur frontmatter. */
  declaredBy: string[];
}

// ===== Serveur distant (remote.rs = camelCase) =====

export interface RemoteStatus {
  running: boolean;
  /** Adresse réellement écoutée, quand le serveur tourne. */
  address?: string | null;
  /** Adresse configurée pour le prochain démarrage. */
  bind: string;
  /** Démarrage automatique au lancement. */
  enabled: boolean;
  tokenConfigured: boolean;
  /** Faux quand le serveur est joignable depuis le réseau. */
  loopbackOnly: boolean;
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
