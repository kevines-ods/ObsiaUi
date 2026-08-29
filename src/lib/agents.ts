/**
 * Chargement des agents depuis le contrat IPC dédié.
 *
 * Le backend (`agents_list`) scanne `IA/agents/*.md`, parse et valide le
 * frontmatter (`schema >= 1`, `kind == agent`, `name`/`description`
 * obligatoires) et trie par `name`. Les fichiers invalides sont ignorés avec un
 * warning côté Rust — l'UI ne reçoit que des agents conformes, sans parsing
 * YAML côté frontend.
 */
import * as ipc from "./ipc";
import type { AgentInfo } from "../types/ipc";

/** Liste les agents du coffre (validés et typés par le backend). */
export async function loadAgents(): Promise<AgentInfo[]> {
  return ipc.agentsList();
}
