---
name: officecli
description: Opérations bureautiques (doc, mail, calendrier).
type: on-demand
sandbox: true
---

# Skill — Office CLI

Automatise les tâches bureautiques via des scripts CLI.

## Procédure
1. Identifier l'outil cible (doc/pdf, email, calendrier).
2. Exécuter dans un **sandbox** isolé.
3. Afficher un preview avant toute action destructive.
4. Archiver l'historique dans `.archive/`.

## Contraintes
- Sandbox obligatoire (ne jamais exécuter hors sandbox).
- Aucune action destructive sans preview + revue.
- Les clés de compte vivent dans `secrets/` (gitignored).
