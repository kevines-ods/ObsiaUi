---
name: cron
description: Tâches planifiées (cron, systemd).
type: on-demand
sandbox: true
---

# Skill — Cron

Planifie des tâches récurrentes (indexation, review, sauvegarde).

## Procédure
1. Définir la tâche, la fréquence, le trigger.
2. Écrire le script dans `IA/scripts/` (Rust/Tauri backend ou shell sandboxé).
3. Enregistrer le cron (`crontab -e` ou `systemd timer`).
4. Loguer chaque exécution dans `IA/system/session-log/`.

## Contraintes
- Sandbox pour tout script exécuté.
- Ne jamais planifier de suppression destructive.
- Prévoir un mode "dry-run".
