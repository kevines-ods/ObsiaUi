# providers.md — Fournisseurs LLM (multi-fournisseur)

Le système supporte plusieurs fournisseurs. L'UI expose :
- un **bouton Fournisseur** + un **menu déroulant** LLM.

## Catalogue
| Fournisseur | Type | Modèle(s) | Notes |
|---|---|---|---|
| OpenAI | API | gpt-... | Payant, rapide |
| Anthropic | API | claude-... | Bonne raisonner |
| Google | API | gemini-... | Multimodal |
| Ollama | Local | llama, gemma | Gratuit, hors-ligne, lent |
| LM Studio | Local | (modèles locaux) | Interface graphique |

## Configuration
Chaque provider est un bloc dans `IA/config/providers/*.md` :
```
{
  "id": "anthropic",
  "type": "api",
  "base_url": "https://api.anthropic.com",
  "models": ["claude-3-5-sonnet-20241022"],
  "default": true,
  "api_key_env": "ANTHROPIC_API_KEY"
}
```

## Interprétation
Bascule par capacité : vision/speech → provider multimodal ; texte → moins cher.

## Sécurité
Les clés API vivent dans `secrets/` (gitignored), **jamais** dans le coffre.
