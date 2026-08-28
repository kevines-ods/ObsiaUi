# RUNTIME.md — Le runtime complet de OBSI

> Un SEUL fichier qui résume tout ce qui a été construit (structure + règles + agents +
> skills + application). C'est le fichier à ouvrir en PREMIER si tu dois changer
> d'outils de construction (ex. abandonner Tauri/Rust) : il suffit de lire ce fichier
> pour reconstruire l'équivalent ailleurs. C'est ton "prompt système" qui te donne le contexte.

## 1. Le concept en 3 lignes
- On construit un **système d'orchestration agentic** (jamais "système d'exploitation").
- L'UI (Tauri/Rust, multi-fournisseur local+API) n'est qu'un **terminal humain** sur le vrai système.
- Le vrai système = le **coffre Obsidian** (`obsi_vault/`), Markdown + rétroliens, Git. Les agents y vivent.

## 2. Les deux coffre (frontière absolue)
| Coffre | Rôle | À faire |
|---|---|---|
| `obsi_vault/` | **Le coffre vivant** — le SEUL endroit où l'on exécute des choses. Mémoire, agents, skills, MCP, scripts, git. | ✅ On y construit tout. |
| `Obsia/` | **Historique / prototype** (ancien projet Obsidian "système d'exploitation"). | ⛔ Ne pas toucher. Lecture seule. |

Rappel : dans `obsi_vault/`, seuls les dossiers `IA/agents/` et `IA/skills/` comptent à ce stade.

## 3. L'application (terminal humain) — Tauri/Rust
- **Multi-fournisseur** : bouton "fournisseur" (choix local ou API) + menu déroulant (sélection LLM).
- **3 zones** : chat / contrôle (réflexions, écritures des agents) / gestionnaire de fichier (coffre Obsidian).
- Les zones contrôle + gestionnaire de fichier sont à gauche/droite et se réduisent.
- L'UI est modifiable via le chat de l'agent "assistant" (patches).

## 4. Le coffre OBSI (obsi_vault/)
- `VAULT.md` — contrat d'exploitation (porte d'entrée humaine + agent). **Lis-le en premier.**
- `README.md` — conventions, démarrage, sécurité.
- `/mémoire/agent N/` — mémoire : `sommaire.md` → `projets K/` → `AAAA-MM-JJ-titre.md` (rétroliens).
- `/IA/agents/*.md` — système prompt + skills/MCP référencés.
- `/IA/skills/*.md` — comportement réutilisable (un skill = une compétence).
- `/IA/MCP/*.md` — outils structurés (lecture seule / revue humaine).
- `/IA/system/` — contrat, index, fournisseurs.
- `scripts/regenerate_sommaire.py` — régénère les `sommaire.md` (jamais à la main).

## 5. Les 3 agents (obsi_vault/IA/agents/)
- **bibliothécaire** (ex obsidian-manager) — indexe le coffre, récupère le contexte, **lecture seule**.
- **développeur** — génère/corrige du code, crée des skills, **patch Git + revue humaine**.
- **assistant de bureau** — doc, mail, calendrier, navigation web (officecli, cron, chrome-devtools).

## 6. Les 6 skills (obsi_vault/IA/skills/)
- **obsidian-manager** — gestion du coffre (recherche, rétroliens, résumés, index), lecture seule.
- **web-research**, **officecli**, **troubleshooting**, **cron**, **skill-créator**.

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
