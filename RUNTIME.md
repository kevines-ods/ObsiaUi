# RUNTIME.md — Le runtime complet de OBSIA

> Un SEUL fichier qui résume tout ce qui a été construit (structure + règles + agents +
> skills + application). C'est le fichier à ouvrir en PREMIER si tu dois changer
> d'outils de construction (ex. abandonner Tauri/Rust) : il suffit de lire ce fichier
> pour reconstruire l'équivalent ailleurs. C'est ton "prompt système" qui te donne le contexte.

## 1. Le concept en 3 lignes
- On construit un **système d'orchestration agentic** (jamais "système d'exploitation").
- L'UI (Tauri/Rust, multi-fournisseur local+API) n'est qu'un **terminal humain** sur le vrai système.
- Le vrai système = le **coffre Obsidian** (`obsia_vault/`), Markdown + rétroliens, Git. Les agents y vivent.

## 2. Les deux coffre (frontière absolue)
| Coffre | Rôle | À faire |
|---|---|---|
| `obsia_vault/` | **Le coffre vivant** — le SEUL endroit où l'on exécute des choses. Mémoire, agents, skills, MCP, scripts, git. | ✅ On y construit tout. |
| `build/` | **Le framework** — UI React + backend Rust/Tauri. Accès réservé à l'agent `assistant` (patch revu). | ✅ Modifiable via le chat (patches). |

Rappel : dans `obsia_vault/`, seuls les dossiers `IA/agents/`, `IA/skills/`,
`IA/MCP/`, `mémoire/`, `scripts/` et `brouillon/` comptent à ce stade.

## 3. L'application (terminal humain) — Tauri/Rust
- **Multi-fournisseur** : bouton "fournisseur" (choix local ou API) + menu déroulant (sélection LLM).
- **3 zones** : chat / contrôle (réflexions, écritures des agents) / gestionnaire de fichier (coffre Obsidian).
- Les zones contrôle + gestionnaire de fichier sont à gauche/droite et se réduisent.
- L'UI est modifiable via le chat de l'agent "assistant" (patches).

## 4. Le coffre OBSIA (obsia_vault/)
- `VAULT.md` — contrat d'exploitation (porte d'entrée humaine + agent). **Lis-le en premier.**
- `README.md` — conventions, démarrage, sécurité.
- `/mémoire/<agent>/<projet>/` — mémoire par **nom d'agent** et **nom de projet** : `sommaire.md` → `AAAA-MM-JJ-titre.md` (rétroliens). Jamais `agent 1`/`projets 2`.
- `/IA/agents/*.md` — système prompt + skills/MCP référencés.
- `/IA/skills/*.md` — comportement réutilisable (un skill = une compétence).
- `/IA/MCP/*.md` — outils structurés (lecture seule / revue humaine).
- `/IA/system/` — contrat, index, fournisseurs.
- `scripts/regenerate_sommaire.py` — régénère les `sommaire.md` (jamais à la main).

## 5. L'agent (obsia_vault/IA/agents/)
- **assistant** — agent de base de l'app : modifie l'UI, ajoute des fonctionnalités, crée des skills. Seul agent autorisé sur `build/` (patch + tests obligatoires).

## 6. Les 12 skills (obsia_vault/IA/skills/)
- **core** : `obsidian-manager` (gestion du coffre), `createur-de-skill` (création de skills).
- **outil** : `bureautique`, `conteneurs-docker`, `cron`, `diagnostic-linux`, `mermaid`, `pdf`, `proxmox`, `remediation-linux`, `sauvegardes`, `traefik`.

## 7. Règles de sécurité (incontournables)
- Accès scoped au coffre uniquement ; lecture seule d'abord, écritures revuées (patches Git).
- **Jamais de suppression** : archiver dans `.archive/`. Jamais d'écriture hors coffre.
- `/IA/MCP/` : modification revue humaine obligatoire.
- Sandbox obligatoire pour toute exécution de code.
- `sommaire.md` = générés (jamais à la main).

## 8. Comment utiliser ce fichier
- Avant de coder quoi que ce soit (choix du framework, de l'orchestrateur, des outils),
  ouvre `RUNTIME.md` + `VAULT.md` + `README.md` : ils contiennent toute la décision à prendre.
- Si tu changes d'outil de construction : tu n'as rien à perdre, car tout est décrit ici.

---
*Fichier à jour au fil du projet. Mis à jour : 2026-08-27.*
