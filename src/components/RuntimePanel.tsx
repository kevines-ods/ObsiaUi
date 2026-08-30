/**
 * Moteurs LLM locaux — résultat de `runtimes_detect`.
 *
 * Affiche ce que la détection a réellement trouvé (adresse retenue, d'où elle
 * vient, modèles servis) plutôt qu'une simple liste de fournisseurs : quand
 * rien ne répond, l'utilisateur a besoin de savoir *pourquoi* — daemon arrêté,
 * mauvais port, ou binaire absent de la machine.
 */
import { useApp } from "../context/AppContext";
import type { DetectedRuntime, RuntimeOrigin } from "../types/ipc";

/** Rend l'origine d'une adresse en une mention courte et lisible. */
function originLabel(origin: RuntimeOrigin): string {
  switch (origin.type) {
    case "config":
      return "configuré";
    case "env":
      return origin.detail;
    case "process":
      return "processus détecté";
    case "defaultPort":
      return "port par défaut";
  }
}

function RuntimeRow({ runtime }: { runtime: DetectedRuntime }): React.JSX.Element {
  const models = runtime.models.length;
  return (
    <div className="runtime-row">
      <span className={`badge ${runtime.reachable ? "badge-ok" : "badge-err"}`}>
        {runtime.reachable ? "actif" : "absent"}
      </span>
      <div className="runtime-body">
        <div className="runtime-title">
          {runtime.label}
          {runtime.version && <span className="runtime-version"> v{runtime.version}</span>}
        </div>
        <div className="runtime-meta">
          <code>{runtime.baseUrl}</code> · {originLabel(runtime.origin)}
        </div>
        {runtime.reachable ? (
          <div className="runtime-meta">
            {models === 0
              ? "aucun modèle chargé"
              : `${models} modèle${models > 1 ? "s" : ""} : ${runtime.models.join(", ")}`}
          </div>
        ) : (
          runtime.error && <div className="runtime-meta">{runtime.error}</div>
        )}
      </div>
    </div>
  );
}

export default function RuntimePanel(): React.JSX.Element {
  const { runtimeScan, detectingRuntimes, detectRuntimes, refreshProviders } = useApp();

  const reachable = runtimeScan?.runtimes.filter((r) => r.reachable) ?? [];
  // Les adresses injoignables ne sont montrées que si RIEN n'a répondu :
  // sinon la liste des ports testés noierait le résultat utile.
  const shown = reachable.length > 0 ? reachable : (runtimeScan?.runtimes ?? []);
  const binaries = runtimeScan?.binaries ?? [];

  const handleDetect = async (): Promise<void> => {
    await detectRuntimes();
    await refreshProviders();
  };

  return (
    <section className="runtime-panel">
      <div className="dropdown-head">
        <span>Moteurs locaux</span>
        <button
          type="button"
          className="link"
          onClick={() => void handleDetect()}
          disabled={detectingRuntimes}
        >
          {detectingRuntimes ? "Détection…" : "Détecter"}
        </button>
      </div>

      {!runtimeScan && !detectingRuntimes && (
        <p className="empty-hint">Aucune détection lancée.</p>
      )}

      {shown.map((runtime) => (
        <RuntimeRow key={`${runtime.providerId}-${runtime.baseUrl}`} runtime={runtime} />
      ))}

      {runtimeScan && reachable.length === 0 && binaries.length > 0 && (
        <p className="empty-hint">
          Installé mais arrêté : {binaries.map((b) => b.name).join(", ")}. Démarrez le
          daemon (<code>ollama serve</code>, <code>llama-server</code>) puis relancez la
          détection.
        </p>
      )}

      {runtimeScan && reachable.length === 0 && binaries.length === 0 && (
        <p className="empty-hint">
          Ni Ollama ni llama.cpp trouvés sur cette machine.
        </p>
      )}
    </section>
  );
}
