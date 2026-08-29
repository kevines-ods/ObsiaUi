# VAULT.md — Contrat d'exploitation du coffre OBSIA

> Porte d'entrée pour l'humain ET pour l'agent "assistant". Lis ce fichier en
> premier avant toute modification.

## 1. À quoi sert ce coffre ?
Mémoire durable + définition d'agents pour un système d'orchestration agentic
natif Linux. Les agents lisent/écrivent ici, ne nulle part ailleurs.

## 2. Structure des dossiers
| Dossier | Rôle |
|---|---|
| `/mémoire/agent N/` | Mémoire d'un agent : sommaire + projets + entrées |
| `/mémoire/agent N/projets K/` | Un projet, indexé par son `sommaire.md` |
| `/mémoire/agent N/projets K/AAAA-MM-JJ-titre.md` | Entrée de projet datée |
| `/IA/agents/*.md` | Définition d'un agent (system prompt + références) |
| `/IA/skills/*.md` | Comportement réutilisable (un skill = une compétence) |
| `/IA/MCP/*.md` | Configuration + doc d'un outil structuré |
| `/IA/system/` | Contrat, index, fournisseurs LLM |

## 3. Conventions de nommage
- Index : `sommaire.md`
- Entrée : `AAAA-MM-JJ-titre.md` (ex. `2026-08-27-lancement-obsia.md`) ou `slug-titre.md`

## 4. Frontières (lecture seule / revue)
- `/IA/MCP/` : modification **revue humaine obligatoire**.
- `.archive/` : zone de quarantaine, jamais écrasée par un agent.
- Les `sommaire.md` sont **générés** à partir des dossiers (voir règle 6).

## 5. Sources & citations
- Une note durable distingue **évidence** (URL source), **interprétation** et
  **synthèse IA**. Garde les URLs à la fin du fichier.

## 6. Règle des rétroliens (automatique)
Chaque `sommaire.md` énumère ses sous-dossiers (ou dossiers sœurs) avec un court
résumé. Un script (`scripts/regenerate_sommaire.py`) les régénère depuis le
système de fichiers. **Ne les écris pas à la main** — sinon le diff Git ne sera
plus fiable.

## 7. Ce que l'agent NE doit jamais faire
- Ne jamais supprimer sans archiver dans `.archive/`.
- Ne jamais écrire hors du coffre.
- Ne jamais modifier `/IA/MCP/` sans preview + revue.
- Ne jamais exécuter de code sans sandbox.

## 8. Log des sessions
À la fin de chaque session active, écris une note dans
`/IA/system/session-log/YYYY-MM-DD.md` (décisions, artefacts changés, questions).
