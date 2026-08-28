---
name: obsidian-manager
description: Gérer le coffre Obsidian (recherche, rétroliens, résumé, index).
type: core
read-only: true
---

# Skill — Obsidian Manager

Gère le coffre Obsidian : recherche, rétroliens, résumés, mise à jour des index.

## Procédure
1. Lire `../system/VAULT-CONTRACT.md`.
2. Pour une requête : localiser le(s) projet(s) via `/mémoire/*/sommaire.md`
   (rétroliens).
3. Extraire le contexte pertinent, **citer les chemins**.
4. Si mise à jour nécessaire : régénérer les `sommaire.md` via
   `scripts/regenerate_sommaire.py` (jamais à la main).

## Outils
- Recherche par texte : `rg "mot" --glob "!*.md"` ou `grep -ri`.
- Rétroliens : lister les fichiers liant vers la cible.
- Résumé : extraire le titre + décisions + statut d'une note.

## Contraintes
- **Lecture seule** : aucun écrit, déplacement ou suppression.
- Ne jamais écraser un fichier sans preview + revue.
