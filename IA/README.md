# /IA/ — Définition des agents, skills et outils

Toutes les définitions d'agents, de compétences (skills) et d'outils structurés
(MCP) vivent ici. C'est la "configuration déclarative" du système.

```
IA/
├── README.md
├── system/              ← contrat + index + fournisseurs (lecture seule)
│   ├── VAULT-CONTRACT.md
│   ├── agents-index.md
│   ├── skills-index.md
│   └── providers.md
├── agents/              ← un fichier .md = un agent
│   ├── bibliothécaire.md
│   ├── développeur.md
│   └── assistant de bureau.md
├── skills/              ← un fichier .md = une compétence
│   ├── obsidian-manager.md
│   ├── web-research.md
│   ├── officecli.md
│   ├── troubleshooting.md
│   ├── cron.md
│   └── skill-créator.md
└── MCP/                 ← un fichier .md = un outil structuré
    ├── chrome-devtools.md
    └── git-hub.md
```

**Format d'un agent** (frontmatter + body) :
```markdown
---
name: assistant de bureau
role: assistant
skills: [officecli, cron]      ← skills activées par défaut
mcp: [chrome-devtools]          ← outils structurés activés
read-only: false
---
# Système prompt
...
```

**Format d'un skill** : frontmatter (`name`, `description`) + règles d'usage +
exemples.

**Format d'un MCP** : description, outils exposés, permissions, sécurité.
