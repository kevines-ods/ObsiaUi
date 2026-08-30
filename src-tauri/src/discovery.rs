//! Détection native Linux des runtimes LLM locaux (Ollama, llama.cpp).
//!
//! Objectif : que l'utilisateur n'ait **rien à configurer**. Au démarrage (et
//! sur demande via `runtimes_detect`), OBSIA cherche les moteurs locaux dans
//! cet ordre de priorité, puis sonde chaque candidat en parallèle :
//!
//! 1. **Config applicative** (`ollamaHost`, `llamacppHost`) — choix explicite.
//! 2. **Variables d'environnement** (`OLLAMA_HOST`, `LLAMA_SERVER_HOST`, …).
//! 3. **Processus en cours** (`/proc`) — on lit `comm`, `cmdline` et `environ`
//!    du daemon pour en déduire l'hôte et le port réellement écoutés. C'est ce
//!    qui permet de trouver un `llama-server --port 9090` non standard.
//! 4. **Ports par défaut** (11434 pour Ollama, 8080/8081/8000 pour llama.cpp).
//!
//! On signale aussi les **binaires installés mais non démarrés** (scan du
//! `PATH`), pour pouvoir dire « Ollama est installé, le daemon est arrêté »
//! plutôt qu'un « aucun modèle » incompréhensible.
//!
//! Toutes les fonctions de dérivation (normalisation d'URL, parsing de ligne
//! de commande, classification de processus) sont **pures et testées** ; seul
//! le sondage HTTP touche au réseau.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, info};

/// Délai maximal d'une sonde HTTP. Volontairement court : la détection est
/// exécutée au démarrage et ne doit jamais faire attendre l'interface.
const PROBE_TIMEOUT: Duration = Duration::from_millis(1200);

/// Port par défaut du daemon Ollama.
pub const OLLAMA_DEFAULT_PORT: u16 = 11434;

/// Ports usuels de `llama-server` (llama.cpp) : 8080 est le défaut amont,
/// 8081 et 8000 sont les rabattements les plus fréquents.
pub const LLAMACPP_DEFAULT_PORTS: &[u16] = &[8080, 8081, 8000];

/// Variables d'environnement consultées pour chaque runtime.
const OLLAMA_ENV_VARS: &[&str] = &["OLLAMA_HOST", "OLLAMA_BASE_URL"];
const LLAMACPP_ENV_VARS: &[&str] = &[
    "LLAMA_SERVER_HOST",
    "LLAMACPP_HOST",
    "LLAMA_CPP_BASE_URL",
    "LLAMA_API_BASE",
];

// ===== Types du contrat =====

/// Famille de runtime local reconnue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeKind {
    Ollama,
    LlamaCpp,
}

impl RuntimeKind {
    /// Identifiant du provider correspondant dans le registry LLM.
    pub fn provider_id(self) -> &'static str {
        match self {
            RuntimeKind::Ollama => "ollama",
            RuntimeKind::LlamaCpp => "llamacpp",
        }
    }

    /// Nom lisible pour l'interface.
    pub fn label(self) -> &'static str {
        match self {
            RuntimeKind::Ollama => "Ollama",
            RuntimeKind::LlamaCpp => "llama.cpp",
        }
    }
}

/// D'où vient un candidat — affiché tel quel dans l'interface pour que
/// l'utilisateur comprenne *pourquoi* OBSIA a choisi cette adresse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type", content = "detail")]
pub enum Origin {
    /// Adresse fixée dans la config applicative.
    Config,
    /// Adresse issue d'une variable d'environnement (nom de la variable).
    Env(String),
    /// Adresse déduite d'un processus en cours (`pid — commande`).
    Process(String),
    /// Port conventionnel du runtime.
    DefaultPort,
}

impl Origin {
    /// Poids de priorité (plus petit = plus prioritaire).
    fn rank(&self) -> u8 {
        match self {
            Origin::Config => 0,
            Origin::Env(_) => 1,
            Origin::Process(_) => 2,
            Origin::DefaultPort => 3,
        }
    }
}

/// Candidat avant sondage : une adresse à tester pour un runtime donné.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub kind: RuntimeKind,
    pub base_url: String,
    pub origin: Origin,
}

/// Runtime effectivement sondé (joignable ou non).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedRuntime {
    pub kind: RuntimeKind,
    /// Identifiant du provider à enregistrer (`ollama`, `llamacpp`).
    pub provider_id: String,
    pub label: String,
    pub base_url: String,
    pub origin: Origin,
    pub reachable: bool,
    /// Version rapportée par le daemon quand il l'expose.
    pub version: Option<String>,
    /// Modèles servis (noms bruts tels que rapportés par le daemon).
    pub models: Vec<String>,
    /// Message d'erreur quand la sonde échoue.
    pub error: Option<String>,
}

/// Binaire trouvé dans le `PATH` (le daemon peut être arrêté).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedBinary {
    pub kind: RuntimeKind,
    pub name: String,
    pub path: String,
}

/// Résultat complet d'un scan.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeScan {
    /// Runtimes sondés, joignables d'abord.
    pub runtimes: Vec<DetectedRuntime>,
    /// Binaires installés (que le daemon tourne ou non).
    pub binaries: Vec<DetectedBinary>,
}

impl RuntimeScan {
    /// Runtimes réellement joignables, dans l'ordre de priorité.
    pub fn reachable(&self) -> impl Iterator<Item = &DetectedRuntime> {
        self.runtimes.iter().filter(|r| r.reachable)
    }

    /// Première adresse joignable pour un runtime donné.
    pub fn first_reachable(&self, kind: RuntimeKind) -> Option<&DetectedRuntime> {
        self.reachable().find(|r| r.kind == kind)
    }
}

// ===== Normalisation d'URL (pure) =====

/// Normalise une adresse hétérogène en URL de base utilisable.
///
/// Accepte `11434`, `127.0.0.1:11434`, `localhost`, `http://x:1/`,
/// `https://gpu.lan:8080/v1` (le suffixe `/v1` est retiré : les providers
/// ajoutent eux-mêmes leur chemin). Renvoie `None` si l'entrée est vide.
pub fn normalize_base_url(raw: &str, default_port: u16) -> Option<String> {
    let raw = raw.trim().trim_end_matches('/');
    if raw.is_empty() {
        return None;
    }

    // Port seul (« 11434 ») : hôte de bouclage implicite.
    if raw.chars().all(|c| c.is_ascii_digit()) {
        return Some(format!("http://127.0.0.1:{raw}"));
    }

    let (scheme, rest) = match raw.split_once("://") {
        Some((s, r)) => (s.to_ascii_lowercase(), r),
        None => ("http".to_string(), raw),
    };
    if scheme != "http" && scheme != "https" {
        return None;
    }

    // On ne garde que l'autorité : le chemin (`/v1`, `/api`) est ajouté par
    // les providers, le conserver produirait des URL en `/v1/v1/...`.
    let authority = rest.split('/').next().unwrap_or(rest);
    if authority.is_empty() {
        return None;
    }

    // Ajout du port par défaut si absent. On ignore le cas IPv6 littéral
    // (`[::1]:8080`) pour la détection du séparateur.
    let has_port = if authority.starts_with('[') {
        authority.rsplit_once(']').map(|(_, p)| p.starts_with(':')) == Some(true)
    } else {
        authority.contains(':')
    };

    if has_port {
        Some(format!("{scheme}://{authority}"))
    } else {
        Some(format!("{scheme}://{authority}:{default_port}"))
    }
}

/// Port par défaut associé à un runtime (le premier de la liste pour llama.cpp).
fn default_port(kind: RuntimeKind) -> u16 {
    match kind {
        RuntimeKind::Ollama => OLLAMA_DEFAULT_PORT,
        RuntimeKind::LlamaCpp => LLAMACPP_DEFAULT_PORTS[0],
    }
}

// ===== Inspection des processus (/proc) =====

/// Un processus local retenu comme daemon LLM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeProcess {
    pub pid: u32,
    pub kind: RuntimeKind,
    /// Commande abrégée, pour l'affichage.
    pub command: String,
    /// URL de base déduite de la ligne de commande / de l'environnement.
    pub base_url: Option<String>,
}

/// Reconnaît un daemon LLM à partir de son `comm` et de sa ligne de commande.
///
/// `comm` est tronqué à 15 caractères par le noyau : on regarde donc aussi
/// l'argv complet (cas `python -m llama_cpp.server`).
pub fn classify_process(comm: &str, cmdline: &[String]) -> Option<RuntimeKind> {
    let comm = comm.trim();
    let argv0 = cmdline
        .first()
        .map(|a| a.rsplit('/').next().unwrap_or(a).to_string())
        .unwrap_or_default();
    let joined = cmdline.join(" ");

    // Ollama : le binaire s'appelle `ollama`, le daemon est `ollama serve`.
    if comm == "ollama" || argv0 == "ollama" {
        // `ollama run …` est un client, pas un serveur : il ne sert pas d'API.
        if cmdline.iter().any(|a| a == "serve") || cmdline.len() <= 1 {
            return Some(RuntimeKind::Ollama);
        }
        return None;
    }

    // llama.cpp : `llama-server` (nom amont actuel), `server` (ancien nom,
    // trop générique seul → on exige un chemin contenant llama), et le binding
    // Python `llama_cpp.server`.
    if comm == "llama-server" || argv0 == "llama-server" || argv0 == "llama-server.bin" {
        return Some(RuntimeKind::LlamaCpp);
    }
    if joined.contains("llama_cpp.server") {
        return Some(RuntimeKind::LlamaCpp);
    }
    if argv0 == "server" && cmdline.first().is_some_and(|a| a.contains("llama")) {
        return Some(RuntimeKind::LlamaCpp);
    }

    None
}

/// Extrait `--host` / `--port` d'une ligne de commande `llama-server`.
///
/// Gère les deux écritures (`--port 9090` et `--port=9090`). On n'accepte
/// **pas** `-p` : dans llama.cpp c'est le prompt, pas le port.
pub fn parse_cmdline_endpoint(cmdline: &[String]) -> (Option<String>, Option<u16>) {
    let mut host = None;
    let mut port = None;
    let mut i = 0;
    while i < cmdline.len() {
        let arg = cmdline[i].as_str();
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) => (f, Some(v.to_string())),
            None => (arg, None),
        };
        let take_value = |i: &mut usize| -> Option<String> {
            if let Some(v) = inline.clone() {
                return Some(v);
            }
            *i += 1;
            cmdline.get(*i).cloned()
        };
        match flag {
            "--host" => host = take_value(&mut i),
            "--port" => port = take_value(&mut i).and_then(|v| v.trim().parse::<u16>().ok()),
            _ => {}
        }
        i += 1;
    }
    (host, port)
}

/// Construit l'URL de base d'un processus llama.cpp depuis son argv.
fn llamacpp_url_from_cmdline(cmdline: &[String]) -> Option<String> {
    let (host, port) = parse_cmdline_endpoint(cmdline);
    if host.is_none() && port.is_none() {
        return None;
    }
    // `0.0.0.0` signifie « toutes les interfaces » : on s'y connecte par
    // bouclage, sans quoi la requête sortirait sur le réseau.
    let host = match host.as_deref() {
        None | Some("0.0.0.0") | Some("::") | Some("*") => "127.0.0.1".to_string(),
        Some(h) => h.to_string(),
    };
    let port = port.unwrap_or(LLAMACPP_DEFAULT_PORTS[0]);
    normalize_base_url(&format!("{host}:{port}"), port)
}

/// Lit une variable dans un bloc `/proc/<pid>/environ` déjà découpé.
pub fn env_value(entries: &[String], key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    entries
        .iter()
        .find_map(|e| e.strip_prefix(&prefix).map(|v| v.to_string()))
}

/// Découpe un buffer séparé par des NUL (`cmdline`, `environ`).
fn split_nul(raw: &[u8]) -> Vec<String> {
    raw.split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).to_string())
        .collect()
}

/// Parcourt `/proc` à la recherche de daemons LLM.
///
/// Hors Linux (ou si `/proc` est inaccessible), renvoie une liste vide : la
/// détection se rabat alors sur l'environnement et les ports par défaut.
pub fn scan_processes() -> Vec<RuntimeProcess> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        debug!("/proc indisponible — détection par processus ignorée");
        return found;
    };

    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        let dir = entry.path();
        let comm = std::fs::read_to_string(dir.join("comm")).unwrap_or_default();
        let cmdline = std::fs::read(dir.join("cmdline"))
            .map(|raw| split_nul(&raw))
            .unwrap_or_default();
        let Some(kind) = classify_process(comm.trim(), &cmdline) else {
            continue;
        };

        // L'environnement du processus n'est lisible que si l'on possède le
        // même UID : l'absence n'est pas une erreur, on se rabat sur l'argv.
        let environ = std::fs::read(dir.join("environ"))
            .map(|raw| split_nul(&raw))
            .unwrap_or_default();

        let base_url = match kind {
            RuntimeKind::Ollama => OLLAMA_ENV_VARS
                .iter()
                .find_map(|v| env_value(&environ, v))
                .and_then(|h| normalize_base_url(&h, OLLAMA_DEFAULT_PORT)),
            RuntimeKind::LlamaCpp => llamacpp_url_from_cmdline(&cmdline).or_else(|| {
                LLAMACPP_ENV_VARS
                    .iter()
                    .find_map(|v| env_value(&environ, v))
                    .and_then(|h| normalize_base_url(&h, LLAMACPP_DEFAULT_PORTS[0]))
            }),
        };

        let command = if cmdline.is_empty() {
            comm.trim().to_string()
        } else {
            cmdline.join(" ")
        };
        found.push(RuntimeProcess {
            pid,
            kind,
            command: truncate(&command, 120),
            base_url,
        });
    }
    found.sort_by_key(|p| p.pid);
    found
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

// ===== Binaires installés (PATH) =====

/// Noms de binaires recherchés dans le `PATH`, par runtime.
const BINARY_NAMES: &[(RuntimeKind, &str)] = &[
    (RuntimeKind::Ollama, "ollama"),
    (RuntimeKind::LlamaCpp, "llama-server"),
    (RuntimeKind::LlamaCpp, "llama-cli"),
];

/// Cherche les binaires des runtimes dans le `PATH` fourni.
///
/// Séparé de la lecture d'environnement pour rester testable sans toucher au
/// `PATH` réel du processus.
pub fn scan_binaries_in(path_var: &str) -> Vec<DetectedBinary> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for dir in path_var.split(':').filter(|d| !d.is_empty()) {
        for (kind, name) in BINARY_NAMES {
            let candidate = PathBuf::from(dir).join(name);
            if !is_executable_file(&candidate) {
                continue;
            }
            // Un même binaire peut apparaître dans plusieurs entrées du PATH.
            if !seen.insert((*kind, *name)) {
                continue;
            }
            out.push(DetectedBinary {
                kind: *kind,
                name: (*name).to_string(),
                path: candidate.to_string_lossy().to_string(),
            });
        }
    }
    out
}

fn is_executable_file(path: &PathBuf) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Binaires trouvés dans le `PATH` du processus courant.
pub fn scan_binaries() -> Vec<DetectedBinary> {
    scan_binaries_in(&std::env::var("PATH").unwrap_or_default())
}

// ===== Construction des candidats (pure) =====

/// Assemble la liste ordonnée et dédupliquée des adresses à sonder.
///
/// `config_hosts` porte les adresses explicites de la config applicative,
/// `env` la vue de l'environnement (injectable pour les tests), `processes`
/// le résultat de [`scan_processes`].
pub fn build_candidates(
    config_hosts: &[(RuntimeKind, String)],
    env: &dyn Fn(&str) -> Option<String>,
    processes: &[RuntimeProcess],
) -> Vec<Candidate> {
    let mut candidates: Vec<Candidate> = Vec::new();

    let mut push = |kind: RuntimeKind, url: Option<String>, origin: Origin| {
        if let Some(base_url) = url {
            candidates.push(Candidate {
                kind,
                base_url,
                origin,
            });
        }
    };

    // 1. Config explicite.
    for (kind, host) in config_hosts {
        push(
            *kind,
            normalize_base_url(host, default_port(*kind)),
            Origin::Config,
        );
    }

    // 2. Variables d'environnement.
    for var in OLLAMA_ENV_VARS {
        push(
            RuntimeKind::Ollama,
            env(var).and_then(|h| normalize_base_url(&h, OLLAMA_DEFAULT_PORT)),
            Origin::Env((*var).to_string()),
        );
    }
    for var in LLAMACPP_ENV_VARS {
        push(
            RuntimeKind::LlamaCpp,
            env(var).and_then(|h| normalize_base_url(&h, LLAMACPP_DEFAULT_PORTS[0])),
            Origin::Env((*var).to_string()),
        );
    }

    // 3. Processus en cours.
    for proc in processes {
        let url = proc.base_url.clone().or_else(|| match proc.kind {
            RuntimeKind::Ollama => normalize_base_url("127.0.0.1", OLLAMA_DEFAULT_PORT),
            RuntimeKind::LlamaCpp => None,
        });
        push(
            proc.kind,
            url,
            Origin::Process(format!("pid {} — {}", proc.pid, proc.command)),
        );
    }

    // 4. Ports conventionnels.
    push(
        RuntimeKind::Ollama,
        normalize_base_url("127.0.0.1", OLLAMA_DEFAULT_PORT),
        Origin::DefaultPort,
    );
    for port in LLAMACPP_DEFAULT_PORTS {
        push(
            RuntimeKind::LlamaCpp,
            normalize_base_url(&format!("127.0.0.1:{port}"), *port),
            Origin::DefaultPort,
        );
    }

    dedupe_candidates(candidates)
}

/// Déduplique par (runtime, URL) en gardant l'origine la plus prioritaire,
/// puis trie par priorité d'origine (config avant port par défaut).
fn dedupe_candidates(mut candidates: Vec<Candidate>) -> Vec<Candidate> {
    candidates.sort_by_key(|c| c.origin.rank());
    let mut seen = HashSet::new();
    candidates.retain(|c| seen.insert((c.kind, c.base_url.clone())));
    candidates
}

// ===== Sondage HTTP =====

/// Sonde un candidat et renvoie le runtime détecté (joignable ou non).
async fn probe(client: &reqwest::Client, candidate: Candidate) -> DetectedRuntime {
    let Candidate {
        kind,
        base_url,
        origin,
    } = candidate;
    let result = match kind {
        RuntimeKind::Ollama => probe_ollama(client, &base_url).await,
        RuntimeKind::LlamaCpp => probe_llamacpp(client, &base_url).await,
    };
    match result {
        Ok((version, models)) => DetectedRuntime {
            kind,
            provider_id: kind.provider_id().to_string(),
            label: kind.label().to_string(),
            base_url,
            origin,
            reachable: true,
            version,
            models,
            error: None,
        },
        Err(e) => DetectedRuntime {
            kind,
            provider_id: kind.provider_id().to_string(),
            label: kind.label().to_string(),
            base_url,
            origin,
            reachable: false,
            version: None,
            models: Vec::new(),
            error: Some(e),
        },
    }
}

/// Ollama : `/api/version` identifie le daemon, `/api/tags` liste les modèles.
async fn probe_ollama(
    client: &reqwest::Client,
    base_url: &str,
) -> Result<(Option<String>, Vec<String>), String> {
    let resp = client
        .get(format!("{base_url}/api/version"))
        .send()
        .await
        .map_err(|e| short_error(&e))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} sur /api/version", resp.status().as_u16()));
    }
    let version = resp
        .json::<serde_json::Value>()
        .await
        .ok()
        .and_then(|v| v["version"].as_str().map(str::to_string));

    let models = client
        .get(format!("{base_url}/api/tags"))
        .send()
        .await
        .ok()
        .and_then(|r| {
            if r.status().is_success() {
                Some(r)
            } else {
                None
            }
        });
    let models = match models {
        Some(r) => r
            .json::<serde_json::Value>()
            .await
            .ok()
            .map(|v| json_string_list(&v["models"], "name"))
            .unwrap_or_default(),
        None => Vec::new(),
    };
    Ok((version, models))
}

/// llama.cpp : `/health` identifie `llama-server`, `/v1/models` liste le ou
/// les modèles chargés. On ne se sert **pas** de `/v1/models` pour identifier
/// le runtime : Ollama expose le même chemin, la confusion serait garantie.
async fn probe_llamacpp(
    client: &reqwest::Client,
    base_url: &str,
) -> Result<(Option<String>, Vec<String>), String> {
    let resp = client
        .get(format!("{base_url}/health"))
        .send()
        .await
        .map_err(|e| short_error(&e))?;
    // `llama-server` répond 503 tant que le modèle charge : c'est bien lui,
    // il n'est simplement pas encore prêt.
    if !resp.status().is_success() && resp.status().as_u16() != 503 {
        return Err(format!("HTTP {} sur /health", resp.status().as_u16()));
    }

    // `/props` expose le chemin du modèle et la version du build.
    let props = client
        .get(format!("{base_url}/props"))
        .send()
        .await
        .ok()
        .and_then(|r| {
            if r.status().is_success() {
                Some(r)
            } else {
                None
            }
        });
    let mut version = None;
    let mut models = Vec::new();
    if let Some(r) = props {
        if let Ok(v) = r.json::<serde_json::Value>().await {
            version = v["build_info"]
                .as_str()
                .or_else(|| v["version"].as_str())
                .map(str::to_string);
            if let Some(path) = v["model_path"]
                .as_str()
                .or_else(|| v["default_generation_settings"]["model"].as_str())
            {
                models.push(model_name_from_path(path));
            }
        }
    }

    if models.is_empty() {
        if let Ok(r) = client.get(format!("{base_url}/v1/models")).send().await {
            if r.status().is_success() {
                if let Ok(v) = r.json::<serde_json::Value>().await {
                    models = json_string_list(&v["data"], "id");
                }
            }
        }
    }
    Ok((version, models))
}

/// Extrait un nom de modèle lisible depuis un chemin de fichier GGUF.
pub fn model_name_from_path(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.strip_suffix(".gguf").unwrap_or(file).to_string()
}

/// Récupère `[{key: "…"}, …]` sous forme de liste de chaînes.
fn json_string_list(value: &serde_json::Value, key: &str) -> Vec<String> {
    value
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m[key].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Message d'erreur court : les erreurs reqwest complètes sont illisibles
/// dans une interface (chaîne de causes + URL répétée).
fn short_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "délai dépassé".to_string()
    } else if e.is_connect() {
        "connexion refusée".to_string()
    } else {
        e.to_string()
    }
}

// ===== Point d'entrée =====

/// Scanne les runtimes locaux : construction des candidats puis sondage
/// **en parallèle** (le coût total est celui de la sonde la plus lente, pas
/// de leur somme).
pub async fn scan(config_hosts: &[(RuntimeKind, String)]) -> RuntimeScan {
    let processes = scan_processes();
    let candidates = build_candidates(
        config_hosts,
        &|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty()),
        &processes,
    );
    debug!(count = candidates.len(), "candidats runtime à sonder");

    let client = reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        // Un daemon local ne passe jamais par un proxy d'entreprise : le
        // respecter ferait échouer la détection sur les postes proxifiés.
        .no_proxy()
        .build()
        .unwrap_or_default();

    let probes = candidates.into_iter().map(|c| probe(&client, c));
    let mut runtimes = futures::future::join_all(probes).await;

    // Joignables d'abord, puis par priorité d'origine.
    runtimes.sort_by_key(|r| (!r.reachable, r.origin.rank()));

    let binaries = scan_binaries();
    info!(
        joignables = runtimes.iter().filter(|r| r.reachable).count(),
        sondes = runtimes.len(),
        binaires = binaries.len(),
        "scan des runtimes locaux terminé"
    );
    RuntimeScan { runtimes, binaries }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== normalize_base_url =====

    #[test]
    fn normalise_les_formes_usuelles() {
        let p = OLLAMA_DEFAULT_PORT;
        assert_eq!(
            normalize_base_url("11434", p).unwrap(),
            "http://127.0.0.1:11434"
        );
        assert_eq!(
            normalize_base_url("127.0.0.1:11434", p).unwrap(),
            "http://127.0.0.1:11434"
        );
        assert_eq!(
            normalize_base_url("localhost", p).unwrap(),
            "http://localhost:11434"
        );
        assert_eq!(
            normalize_base_url("http://gpu.lan:8080/", p).unwrap(),
            "http://gpu.lan:8080"
        );
        assert_eq!(
            normalize_base_url("https://gpu.lan", p).unwrap(),
            "https://gpu.lan:11434"
        );
    }

    #[test]
    fn retire_le_chemin_pour_ne_garder_que_l_autorite() {
        // Sinon les providers produiraient des URL en /v1/v1/chat/completions.
        assert_eq!(
            normalize_base_url("http://gpu.lan:8080/v1", 8080).unwrap(),
            "http://gpu.lan:8080"
        );
    }

    #[test]
    fn refuse_les_entrees_invalides() {
        assert!(normalize_base_url("", 8080).is_none());
        assert!(normalize_base_url("   ", 8080).is_none());
        assert!(normalize_base_url("ftp://x", 8080).is_none());
    }

    #[test]
    fn conserve_le_port_d_une_ipv6_litterale() {
        assert_eq!(
            normalize_base_url("http://[::1]:8080", 11434).unwrap(),
            "http://[::1]:8080"
        );
        assert_eq!(
            normalize_base_url("http://[::1]", 11434).unwrap(),
            "http://[::1]:11434"
        );
    }

    // ===== classify_process =====

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn reconnait_le_daemon_ollama() {
        assert_eq!(
            classify_process("ollama", &argv(&["/usr/local/bin/ollama", "serve"])),
            Some(RuntimeKind::Ollama)
        );
    }

    #[test]
    fn ignore_le_client_ollama_run() {
        // `ollama run` est un client interactif : il n'expose aucune API.
        assert_eq!(
            classify_process("ollama", &argv(&["ollama", "run", "llama3"])),
            None
        );
    }

    #[test]
    fn reconnait_llama_server() {
        assert_eq!(
            classify_process(
                "llama-server",
                &argv(&["/opt/llama.cpp/llama-server", "--port", "9090"])
            ),
            Some(RuntimeKind::LlamaCpp)
        );
        assert_eq!(
            classify_process("python3", &argv(&["python3", "-m", "llama_cpp.server"])),
            Some(RuntimeKind::LlamaCpp)
        );
    }

    #[test]
    fn ignore_un_serveur_generique() {
        // `server` seul est bien trop générique pour être revendiqué.
        assert_eq!(
            classify_process("server", &argv(&["/usr/bin/server"])),
            None
        );
        assert_eq!(
            classify_process(
                "server",
                &argv(&["/opt/llama.cpp/server", "--port", "8080"])
            ),
            Some(RuntimeKind::LlamaCpp)
        );
    }

    // ===== parse_cmdline_endpoint =====

    #[test]
    fn lit_host_et_port_separes_ou_colles() {
        let (h, p) = parse_cmdline_endpoint(&argv(&[
            "llama-server",
            "--host",
            "0.0.0.0",
            "--port",
            "9090",
        ]));
        assert_eq!(h.as_deref(), Some("0.0.0.0"));
        assert_eq!(p, Some(9090));

        let (h, p) = parse_cmdline_endpoint(&argv(&["llama-server", "--host=::1", "--port=8081"]));
        assert_eq!(h.as_deref(), Some("::1"));
        assert_eq!(p, Some(8081));
    }

    #[test]
    fn n_interprete_pas_p_comme_un_port() {
        // Dans llama.cpp, `-p` est le prompt : le confondre avec --port
        // enverrait les requêtes sur un port arbitraire.
        let (_, p) = parse_cmdline_endpoint(&argv(&["llama-server", "-p", "8080"]));
        assert_eq!(p, None);
    }

    #[test]
    fn rabat_l_ecoute_universelle_sur_le_bouclage() {
        // `--host 0.0.0.0` veut dire « toutes les interfaces » ; s'y connecter
        // littéralement enverrait la requête hors de la machine.
        let url = llamacpp_url_from_cmdline(&argv(&[
            "llama-server",
            "--host",
            "0.0.0.0",
            "--port",
            "9090",
        ]));
        assert_eq!(url.as_deref(), Some("http://127.0.0.1:9090"));
    }

    #[test]
    fn pas_d_url_sans_indice_dans_l_argv() {
        assert_eq!(llamacpp_url_from_cmdline(&argv(&["llama-server"])), None);
    }

    // ===== env_value =====

    #[test]
    fn lit_une_variable_du_bloc_environ() {
        let env = argv(&["PATH=/usr/bin", "OLLAMA_HOST=127.0.0.1:12345", "HOME=/root"]);
        assert_eq!(
            env_value(&env, "OLLAMA_HOST").as_deref(),
            Some("127.0.0.1:12345")
        );
        assert_eq!(env_value(&env, "ABSENTE"), None);
    }

    // ===== build_candidates =====

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn les_defauts_sont_toujours_proposes() {
        let candidates = build_candidates(&[], &no_env, &[]);
        assert!(candidates
            .iter()
            .any(|c| c.kind == RuntimeKind::Ollama && c.base_url == "http://127.0.0.1:11434"));
        assert!(candidates
            .iter()
            .any(|c| c.kind == RuntimeKind::LlamaCpp && c.base_url == "http://127.0.0.1:8080"));
    }

    #[test]
    fn la_config_prime_sur_l_environnement_et_le_defaut() {
        let env = |k: &str| (k == "OLLAMA_HOST").then(|| "127.0.0.1:11434".to_string());
        let candidates =
            build_candidates(&[(RuntimeKind::Ollama, "gpu.lan:11434".into())], &env, &[]);
        let first = candidates
            .iter()
            .find(|c| c.kind == RuntimeKind::Ollama)
            .unwrap();
        assert_eq!(first.base_url, "http://gpu.lan:11434");
        assert_eq!(first.origin, Origin::Config);
    }

    #[test]
    fn une_meme_url_n_est_sondee_qu_une_fois_avec_la_meilleure_origine() {
        // Config et défaut pointent la même adresse : un seul candidat, marqué
        // « config » (l'origine la plus prioritaire).
        let candidates = build_candidates(
            &[(RuntimeKind::Ollama, "127.0.0.1:11434".into())],
            &no_env,
            &[],
        );
        let ollama: Vec<_> = candidates
            .iter()
            .filter(|c| c.base_url == "http://127.0.0.1:11434")
            .collect();
        assert_eq!(ollama.len(), 1);
        assert_eq!(ollama[0].origin, Origin::Config);
    }

    #[test]
    fn un_processus_sur_port_exotique_devient_candidat() {
        let processes = vec![RuntimeProcess {
            pid: 4242,
            kind: RuntimeKind::LlamaCpp,
            command: "llama-server --port 9090".into(),
            base_url: Some("http://127.0.0.1:9090".into()),
        }];
        let candidates = build_candidates(&[], &no_env, &processes);
        let found = candidates
            .iter()
            .find(|c| c.base_url == "http://127.0.0.1:9090")
            .expect("le port exotique doit être sondé");
        assert!(matches!(found.origin, Origin::Process(_)));
    }

    // ===== binaires =====

    #[test]
    fn trouve_un_binaire_executable_du_path() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("ollama");
        std::fs::write(&bin, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let found = scan_binaries_in(dir.path().to_str().unwrap());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, RuntimeKind::Ollama);
    }

    #[test]
    fn ignore_un_fichier_non_executable() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ollama"), b"pas un binaire").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                dir.path().join("ollama"),
                std::fs::Permissions::from_mode(0o644),
            )
            .unwrap();
            assert!(scan_binaries_in(dir.path().to_str().unwrap()).is_empty());
        }
    }

    // ===== divers =====

    #[test]
    fn nomme_le_modele_depuis_son_chemin_gguf() {
        assert_eq!(
            model_name_from_path("/models/Qwen3-8B-Q4_K_M.gguf"),
            "Qwen3-8B-Q4_K_M"
        );
        assert_eq!(model_name_from_path("modele"), "modele");
    }

    #[test]
    fn extrait_une_liste_de_chaines_json() {
        let v = serde_json::json!([{"name": "a"}, {"name": "b"}, {"autre": "c"}]);
        assert_eq!(json_string_list(&v, "name"), vec!["a", "b"]);
        assert!(json_string_list(&serde_json::Value::Null, "name").is_empty());
    }
}
