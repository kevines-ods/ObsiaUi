/**
 * Types dérivés du coffre — agents définis dans `IA/agents/*.md`.
 *
 * Un agent est un fichier Markdown avec un frontmatter YAML minimal :
 *   schema, kind, name, description, skills, mcp, read_only.
 */

export interface AgentInfo {
  /** Chemin relatif dans le coffre, ex. `IA/agents/assistant.md`. */
  path: string;
  /** `name` du frontmatter (minuscules, tirets, sans espaces). */
  name: string;
  /** `description` du frontmatter. */
  description: string;
  /** `skills` : compétences activées pour cet agent. */
  skills: string[];
  /** `mcp` : outils structurés activés. */
  mcp: string[];
  /** `read_only` : lecture seule absolue quand `true`. */
  readOnly: boolean;
}
