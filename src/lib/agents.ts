/**
 * Chargement des agents depuis le coffre (`vault_list` + `vault_read`).
 *
 * Les agents sont des notes Markdown sous `IA/agents/*.md` (champ `kind: agent`,
 * frontmatter `name`/`description`/`skills`/`mcp`/`read_only`). Le backend
 * autorise la lecture de `IA/` mais bloque son écriture, ce qui correspond au
 * caractère de référence « en lecture » des agents.
 */
import * as ipc from "./ipc";
import { parseFrontmatter } from "./frontmatter";
import type { AgentInfo } from "../types/agent";

const AGENTS_PREFIX = "IA/agents/";

/** Lie et parse tous les agents du coffre. Ignore silencieusement les fichiers invalides. */
export async function loadAgents(): Promise<AgentInfo[]> {
  const notes = await ipc.vaultList();
  const agentPaths = notes
    .map((n) => n.path)
    .filter((p) => p.startsWith(AGENTS_PREFIX) && p.endsWith(".md"));

  const agents: AgentInfo[] = [];

  for (const path of agentPaths) {
    try {
      const markdown = await ipc.vaultRead(path);
      const fm = parseFrontmatter(markdown);

      const fallbackName = path.slice(AGENTS_PREFIX.length).replace(/\.md$/, "");
      const name = typeof fm.name === "string" && fm.name ? fm.name : fallbackName;

      agents.push({
        path,
        name,
        description: typeof fm.description === "string" ? fm.description : "",
        skills: Array.isArray(fm.skills) ? fm.skills.filter((s) => typeof s === "string") : [],
        mcp: Array.isArray(fm.mcp) ? fm.mcp.filter((s) => typeof s === "string") : [],
        readOnly: fm.read_only === true,
      });
    } catch {
      // Fichier non lisible ou frontmatter invalide : ignoré.
    }
  }

  agents.sort((a, b) => a.name.localeCompare(b.name));
  return agents;
}
