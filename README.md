# ObsiaUi

Interface graphique native Linux pour le coffre [OBSIA](https://github.com/kevines-ods/OBSIA).

Le coffre décrit *quoi* faire — agents, skills, mémoire, en Markdown. ObsiaUi
fournit *avec quoi* : le choix du modèle, l'exécution, le streaming, les
sessions. Le coffre ne connaît pas cette interface et fonctionne sans elle.

## Pile

Tauri 2 (Rust) + React 19 (Vite). Un seul binaire, pas de service de fond, pas
de compte à créer.

## Fournisseurs

**Locaux, détectés sans configuration** — Ollama et llama.cpp. La détection
regarde, dans cet ordre : la configuration de l'application, les variables
d'environnement, les processus en cours (`/proc`, ce qui permet de trouver un
`llama-server --port 9090`), puis les ports conventionnels. Elle signale aussi
les binaires installés dont le daemon est arrêté.

**Par API** — OpenAI, Anthropic, Google Gemini, OpenRouter. Une clé est lue
depuis la variable d'environnement correspondante, sinon depuis la
configuration applicative (fichier `0600`, hors dépôt). Aucune clé n'entre
jamais dans le dépôt ni dans le coffre.

## Développement

```bash
# Dépendances système (Debian/Ubuntu)
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev \
                 librsvg2-dev libssl-dev build-essential pkg-config

npm --prefix src ci
cargo tauri dev
```

Le coffre est cherché dans cet ordre : `OBSIA_VAULT_PATH`, le chemin donné dans
la configuration, puis les emplacements usuels — dont `../OBSIA/obsia_vault`
quand les deux dépôts sont clonés côte à côte.

## Vérifications

```bash
cargo fmt --all && cargo clippy --all-targets && cargo test
npm --prefix src run lint && npm --prefix src run build
```

Les tests d'intégration qui exigent un daemon local sont marqués `#[ignore]` :
`cargo test -- --ignored`.

## Frontière

ObsiaUi n'écrit dans le coffre que dans `brouillon/`. Tout le reste est en
lecture seule : les modifications durables passent par un patch Git revu, comme
l'exige `IA/system/VAULT-CONTRACT.md` du coffre.
