---
name: troubleshooting
description: Diagnostic méthodique de pannes.
type: on-demand
---

# Skill — Troubleshooting

Diagnostic structuré, du plus probable au plus rare.

## Méthode (5 étapes)
1. **Reproduire** le bug de façon déterministe.
2. **Isoler** le composant fautif (coffre / runtime / provider / UI).
3. **Hypothèses** ordonnées par probabilité.
4. **Tester** une hypothèse à la fois.
5. **Documenter** la cause et le patch dans la note du projet.

## Contraintes
- Ne jamais modifier `secrets/` ou `../system/` sans revue.
- Chaque hypothèse = une entrée dans le rapport.
