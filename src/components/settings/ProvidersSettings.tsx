/**
 * Fournisseurs : santé, modèles et clés d'API.
 *
 * Une clé saisie ici part vers la configuration du backend, qui l'écrit dans
 * un fichier en 0600 hors du dépôt. Elle n'est jamais relue par l'interface :
 * seule sa présence remonte.
 */
import { useState } from "react";

import { useApp } from "../../context/AppContext";

export default function ProvidersSettings(): React.JSX.Element {
  const {
    config,
    providers,
    health,
    testingId,
    testProvider,
    saveConfig,
    refreshProviders,
    detectRuntimes,
    detectingRuntimes,
  } = useApp();

  const [cles, setCles] = useState<Record<string, string>>({});
  const [enCours, setEnCours] = useState<string | null>(null);
  const configurees = config?.apiKeysConfigured ?? [];

  const enregistrerCle = async (providerId: string): Promise<void> => {
    const apiKey = cles[providerId]?.trim();
    if (!apiKey) return;
    setEnCours(providerId);
    await saveConfig({ setApiKey: { providerId, apiKey } });
    setCles((prev) => ({ ...prev, [providerId]: "" }));
    setEnCours(null);
    await refreshProviders();
  };

  const oublierCle = async (providerId: string): Promise<void> => {
    setEnCours(providerId);
    await saveConfig({ setApiKey: { providerId, apiKey: "" } });
    setEnCours(null);
    await refreshProviders();
  };

  return (
    <div className="settings-body">
      <div className="team-actions">
        <button
          type="button"
          className="btn btn-mini"
          onClick={() => void detectRuntimes().then(refreshProviders)}
          disabled={detectingRuntimes}
        >
          {detectingRuntimes ? "Détection…" : "Détecter les moteurs locaux"}
        </button>
      </div>

      {providers.length === 0 && (
        <p className="empty-hint">Aucun fournisseur enregistré.</p>
      )}

      {providers.map((provider) => {
        const h = health[provider.id];
        const aUneCle = configurees.includes(provider.id);
        // Les moteurs locaux n'ont pas de clé : afficher un champ vide pour
        // Ollama laisserait croire qu'il en faut une.
        const local = provider.id === "ollama" || provider.id === "llamacpp";
        return (
          <section className="provider-block" key={provider.id}>
            <div className="cp-row">
              <span className="cp-label">{provider.name}</span>
              <button
                type="button"
                className="btn btn-mini"
                onClick={() => void testProvider(provider.id)}
                disabled={testingId === provider.id}
              >
                {testingId === provider.id ? "…" : "Tester"}
              </button>
              {h && (
                <span className={`badge ${h.ok ? "badge-ok" : "badge-err"}`}>
                  {h.ok ? "OK" : "erreur"}
                </span>
              )}
            </div>

            {h && !h.ok && h.error && <p className="runtime-meta">{h.error}</p>}

            <p className="runtime-meta">
              {provider.models.length === 0
                ? "aucun modèle"
                : `${provider.models.length} modèle(s) : ${provider.models
                    .slice(0, 4)
                    .map((m) => m.name || m.id)
                    .join(", ")}${provider.models.length > 4 ? "…" : ""}`}
            </p>

            {!local && (
              <div className="cp-row key-row">
                {aUneCle ? (
                  <>
                    <span className="badge badge-ok">clé configurée</span>
                    <button
                      type="button"
                      className="btn btn-mini"
                      onClick={() => void oublierCle(provider.id)}
                      disabled={enCours === provider.id}
                    >
                      Oublier
                    </button>
                  </>
                ) : (
                  <>
                    <input
                      type="password"
                      placeholder="Clé API…"
                      value={cles[provider.id] ?? ""}
                      onChange={(e) =>
                        setCles((prev) => ({ ...prev, [provider.id]: e.target.value }))
                      }
                    />
                    <button
                      type="button"
                      className="btn btn-mini"
                      onClick={() => void enregistrerCle(provider.id)}
                      disabled={enCours === provider.id || !cles[provider.id]?.trim()}
                    >
                      Enregistrer
                    </button>
                  </>
                )}
              </div>
            )}
          </section>
        );
      })}

      <p className="empty-hint">
        Une variable d'environnement (<code>ANTHROPIC_API_KEY</code>,
        <code> OPENAI_API_KEY</code>…) prime sur la clé enregistrée ici.
      </p>
    </div>
  );
}
