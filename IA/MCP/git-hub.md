---
name: git-hub
description: Push/pull, PR et review sur GitHub.
type: tool
permission: elevated
---

# MCP — Git Hub

Exposé par `@modelcontextprotocol/server-github`.

## Outils exposés
- `repo.get_commits` — historique des patches.
- `repo.create_pull_request` — ouvrir une PR de review.
- `repo.list_issues` — suivi des bugs/features.
- `repo.get_contents` — lire des fichiers distants.

## Permissions
- **Élevées** : écriture sur un dépôt distant.
- Utiliser un dépôt **spécial** (pas le coffre personnel) pour les patches.

## Sécurité
- Jamais de clé GitHub dans le coffre (voir `secrets/`, gitignored).
- Les patches de l'agent sont toujours en PR → **revue humaine obligatoire**.
