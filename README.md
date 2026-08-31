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

## Ce que fait le harness

**Sessions multiples.** Plusieurs conversations persistantes, qui peuvent
répondre en même temps. Chacune porte son agent, son fournisseur et son
modèle. Une session peut être exportée en note Markdown dans `brouillon/` du
coffre, prête pour une revue.

**Équipes d'agents.** Plusieurs agents du coffre travaillent sur un objectif
commun, chacun avec **son propre modèle** — un rôle bavard en local, un rôle
décisif sur un modèle plus capable. Tour de rôle, ou superviseur qui
distribue la parole. Le nombre de tours est borné.

**Plans.** Un objectif découpé en étapes assignées, avec dépendances. Les
étapes indépendantes s'exécutent **en parallèle** ; chacune ne reçoit que
l'objectif et le résultat de ses dépendances. Un plan interrompu reprend là où
il s'est arrêté. Un modèle peut proposer le découpage, qui est relu avant
exécution.

**Sessions à distance.** Cette instance peut exposer son harness à un autre
poste : sessions, équipes et plans s'exécutent sur l'hôte, avec le même flux
d'événements. Jeton obligatoire, écoute locale par défaut, liste blanche de
commandes — la configuration de l'hôte et ses clés d'API ne sont pas exposées.

**Extensions.** Deux niveaux. Un *patch* décrit un thème et une disposition,
sans exécuter de code ; ses valeurs CSS sont validées. Un *plugin* exécute du
JavaScript à des points d'accroche déclarés — voir `exemples/plugin-compteur/`.
Un plugin est inactif à l'installation, et toute modification de son fichier le
redésactive jusqu'à réapprobation.

> Les permissions d'un plugin bornent l'API qu'ObsiaUi lui tend, **pas** ce que
> son code peut atteindre dans la page. N'activez que ce que vous avez lu.

## Lancer

Il faut Rust (`rustup`), Node 20 ou plus, et les bibliothèques GTK/WebKit du
système.

```bash
# Dépendances système — Debian, Ubuntu
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev \
                 librsvg2-dev libssl-dev build-essential pkg-config

# Dépendances système — Fedora
sudo dnf install webkit2gtk4.1-devel gtk3-devel libsoup3-devel \
                 librsvg2-devel openssl-devel @development-tools

# Dépendances système — Arch
sudo pacman -S webkit2gtk-4.1 gtk3 libsoup3 librsvg openssl base-devel
```

Puis, depuis la racine du dépôt :

```bash
npm --prefix src ci
npm --prefix src run build
cargo run --release
```

La fenêtre s'ouvre. Le premier `cargo run` compile tout et prend plusieurs
minutes ; les suivants sont immédiats. Aucun outil supplémentaire n'est
nécessaire : l'interface compilée est embarquée dans le binaire.

Le coffre est cherché dans cet ordre : `OBSIA_VAULT_PATH`, le chemin donné dans
la configuration, puis les emplacements usuels — dont `../OBSIA/obsia_vault`
quand les deux dépôts sont clonés côte à côte. Pour désigner un autre
emplacement :

```bash
OBSIA_VAULT_PATH=/chemin/vers/mon/coffre cargo run --release
```

## Développement

Le rechargement à chaud et la fabrication des paquets passent par le CLI
Tauri, qui n'est pas une dépendance du dépôt et s'installe une fois :

```bash
cargo install tauri-cli --locked --version "^2.0"

cargo tauri dev     # rechargement à chaud du front
cargo tauri build   # paquets Linux dans target/release/bundle/
```

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
