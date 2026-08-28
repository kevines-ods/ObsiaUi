---
name: skill-créator
description: Créer de nouveaux skills (méta-skill).
type: meta
---

# Skill — Skill Creator

Crée de nouveaux skills à partir d'un besoin répétable.

## Méthodologie
1. **Identifier** la tâche répétée (référence, script, règles, exemples).
2. **Définir** les garde-fous (ce que le skill ne doit jamais faire).
3. **Écrire** `IA/skills/<nom>.md` : frontmatter + règles + exemples.
4. **Évaluer** sur des cas de test (bon + mauvais).
5. **Intégrer** dans l'index `IA/system/skills-index.md`.

## Format attendu
```markdown
---
name: <nom>
description: <en une phrase>
type: on-demand
---
# Skill — <Nom>
## Procédure
...
## Contraintes
...
```

## Contraintes
- Un skill = une compétence, **narrow**.
- Garder le cœur platform-agnostic, adapter les wrappers client.
