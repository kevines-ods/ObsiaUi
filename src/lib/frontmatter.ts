/**
 * Parseur minimal de frontmatter YAML (bloc `--- ... ---`).
 *
 * Gère le sous-ensemble utilisé par les agents / skills du coffre :
 *  - scalaires : `key: valeur` (chaîne, booléen, nombre)
 *  - listes : `key:` suivi de lignes `  - item`
 *
 * Pas de dépendance YAML (aucune installée) : suffisant pour l'index d'agents.
 */

export type FrontmatterValue = string | number | boolean | string[];

export type Frontmatter = Record<string, FrontmatterValue>;

function coerce(raw: string): string | number | boolean {
  const value = raw.trim();
  if (value === "true") return true;
  if (value === "false") return false;
  if (value === "null" || value === "~") return "";
  const num = Number(value);
  if (value !== "" && !Number.isNaN(num)) return num;
  return value.replace(/^['"]|['"]$/g, "");
}

/** Extrait et parse le bloc frontmatter ; retourne `{}` si absent/mal formé. */
export function parseFrontmatter(markdown: string): Frontmatter {
  const match = markdown.match(/^---\r?\n([\s\S]*?)\r?\n---/);
  if (!match) return {};

  const out: Frontmatter = {};
  let currentListKey: string | null = null;

  for (const rawLine of match[1].split(/\r?\n/)) {
    const indented = rawLine.length - rawLine.trimStart().length;
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) continue;

    // Élément de liste indenté : `  - item`
    if (indented > 0 && line.startsWith("-") && currentListKey !== null) {
      const item = line.replace(/^-\s*/, "").replace(/^['"]|['"]$/g, "");
      const existing = out[currentListKey];
      out[currentListKey] = Array.isArray(existing) ? [...existing, item] : [item];
      continue;
    }

    const kv = line.match(/^([A-Za-z0-9_-]+):\s*(.*)$/);
    if (!kv) {
      currentListKey = null;
      continue;
    }

    const key = kv[1];
    const value = kv[2].trim();
    currentListKey = null;

    if (value === "") {
      // Tête de liste : un tableau sera rempli par les lignes indentées suivantes.
      out[key] = [];
      currentListKey = key;
      continue;
    }

    if (value.startsWith("-")) {
      // Liste en ligne : `key: - a - b` (rare, on normalise en tableau).
      const items = value
        .split("-")
        .map((s) => s.trim())
        .filter(Boolean);
      out[key] = items;
      continue;
    }

    out[key] = coerce(value);
  }

  return out;
}
