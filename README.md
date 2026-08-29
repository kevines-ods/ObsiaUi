# 🧠 OBSIA — Obsidian Orchestrated System Intelligence

Système d'**orchestration agentic** natif Linux (Tauri/Rust), multi-fournisseur
(local + API), dont la mémoire et la création d'agents reposent entièrement sur
un **coffre Obsidian** (Markdown + rétroliens).

## Démarrage
```bash
git init && git add . && git commit -m "chore: baseline coffre OBSIA"
```

## Conventions
- **Racine = coffre Obsidian.** Tout est Markdown, portable, inspectable, Git.
- **Nom des fichiers** : `sommaire.md` pour les index, `AAAA-MM-JJ-titre.md` ou
  `slug-titre.md` pour les entrées de projet.
- **Rétroliens** : chaque `sommaire.md` énumère et décrit ses dossiers. C'est le
  moteur de découverte de contexte.
- **Frontières** : ce qui est en `lecture seule` ou dans `/IA/MCP` n'est pas
  modifiable par les agents sans revue humaine.

## Structure
- `VAULT.md` — contrat d'exploitation (porte d'entrée humaine + agent)
- `/mémoire/` — mémoire par agent → projets → entrées datées
- `/IA/agents/` — agents (system prompt + skills/MCP référencés)
- `/IA/skills/` — comportement (comment l'agent doit travailler)
- `/IA/MCP/` — outils structurés (chrome-devtools, git-hub…)
- `/IA/system/` — contrat, index, fournisseurs

## Sécurité
1. Accès scoped au coffre uniquement.
2. Lecture seule d'abord, puis écritures revuées (patches Git).
3. Suppression désactivée → archive dans `.archive/`.
