# VAULT-CONTRACT.md — Contrat du système OBSI

Porte d'entrée pour l'agent "assistant" (méta-agent capable de modifier le
système). Lis ceci en premier.

## 1. Nature du système
Système d'**orchestration agentic** natif Linux (Tauri/Rust), multi-fournisseur
(local + API), dont la mémoire = coffre Obsidian.

## 2. Rôles
- **assistant** (méta-agent) — peut modifier l'interface, ajouter des fonctionnalités
  via des **patches** (revus), orchestrer des équipes d'agents.
- **bibliothécaire** — indexe le coffre, récupère le contexte via les rétroliens.
- **développeur** — génère/patche du code.
- **assistant de bureau** — tâches bureautiques + automatisation.

## 3. Flux de modification (patch)
1. L'agent propose une modification.
2. Elle est écrite dans un fichier cible (config/README/sommaire non-généré).
3. Un commit Git = le patch.
4. **Revue humaine obligatoire** avant acceptation.
5. Accepté → fusionné en branche principale.

## 4. Fournisseurs LLM (choix humain)
Voir `providers.md`. Le seul réglage humain en UI = fournisseur + modèle.

## 5. Frontières de sécurité
- `/IA/system/` : **lecture seule** (sauf revue humaine).
- `.archive/` : quarantaine, jamais écrasée.
- Aucun secret dans le coffre (voir `.gitignore`).
