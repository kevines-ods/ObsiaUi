---
name: chrome-devtools
description: Navigation, capture et automatisation web via Chrome DevTools Protocol.
type: tool
permission: elevated
---

# MCP — Chrome DevTools

Exposé par un serveur MCP (ex. `@modelcontextprotocol/server-chrome`).

## Outils exposés
- `Page.navigate` — ouvrir une URL.
- `Page.captureScreenshot` — capture visuelle.
- `Network.getResponseBody` — extraire le contenu d'une page.
- `Runtime.evaluate` — exécuter du JS dans la page (sandboxé).

## Permissions
- **Élevées** : accès à la navigation web et au réseau.
- Ne jamais exposer `Runtime.evaluate` sans sandbox.

## Sécurité
- Ne naviguer que vers des URLs autorisées (listes blanches optionnelles).
- Isolér le profil Chrome en mode utilisateur.
- Loguer chaque navigation dans `.audit/`.
