/**
 * Sélecteur de fournisseur + modèle.
 *
 * - Liste les fournisseurs via `providers_list` (via AppContext).
 * - Affiche les modèles de chaque fournisseur.
 * - Bouton de test de santé via `provider_test`.
 * - La sélection est persistée : `defaultProvider` via `config_set`.
 */
import { useEffect, useState } from "react";

import { useApp } from "../context/AppContext";
import RuntimePanel from "./RuntimePanel";

export default function ProviderSelector(): React.JSX.Element {
  const {
    providers,
    loadingProviders,
    loadError,
    selectedProviderId,
    selectedModel,
    selectProviderAndModel,
    testProvider,
    health,
    testingId,
    refreshProviders,
  } = useApp();

  const [open, setOpen] = useState(false);

  const selected = providers.find((p) => p.id === selectedProviderId);

  // Ferme le dropdown si la sélection change / hors focus.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  const handleTest = async (providerId: string): Promise<void> => {
    if (testingId) return;
    await testProvider(providerId);
  };

  return (
    <div className="provider-selector">
      <button
        type="button"
        className="btn btn-ghost"
        onClick={() => setOpen((v) => !v)}
        disabled={loadingProviders}
        aria-haspopup="listbox"
        aria-expanded={open}
        data-testid="provider-selector-trigger"
      >
        {loadingProviders ? "Chargement…" : selected?.name ?? "Fournisseur"} —{" "}
        {selectedModel || "…"}
      </button>

      {open && (
        <div className="provider-dropdown" role="listbox" aria-label="Fournisseurs et modèles">
          <RuntimePanel />

          <div className="dropdown-head">
            <span>Fournisseurs</span>
            <button
              type="button"
              className="link"
              onClick={() => void refreshProviders()}
            >
              Rafraîchir
            </button>
          </div>

          {loadError && <p className="err-text">{loadError}</p>}

          {providers.length === 0 && !loadingProviders && (
            <p className="empty-hint">Aucun fournisseur configuré.</p>
          )}

          {providers.map((provider) => {
            const h = health[provider.id];
            const isActive = provider.id === selectedProviderId;
            return (
              <div className="provider-group" key={provider.id}>
                <div className="provider-row">
                  <label className="provider-name">
                    <input
                      type="radio"
                      name="provider"
                      checked={isActive}
                      onChange={() => {
                        const model = provider.models[0]?.id ?? "";
                        selectProviderAndModel(provider.id, model);
                      }}
                    />
                    <span>{provider.name}</span>
                  </label>
                  <button
                    type="button"
                    className="btn btn-mini"
                    onClick={() => void handleTest(provider.id)}
                    disabled={testingId === provider.id}
                    title="Tester la connexion"
                  >
                    {testingId === provider.id ? "…" : "Test"}
                  </button>
                  {h && (
                    <span className={`badge ${h.ok ? "badge-ok" : "badge-err"}`}>
                      {h.ok ? "OK" : "Erreur"}
                    </span>
                  )}
                </div>

                {isActive && provider.models.length > 0 && (
                  <div className="model-list">
                    {provider.models.map((model) => (
                      <button
                        type="button"
                        key={model.id}
                        className={model.id === selectedModel ? "active" : ""}
                        onClick={() => {
                          selectProviderAndModel(provider.id, model.id);
                          setOpen(false);
                        }}
                        title={`${model.id} — ctx ${model.context_window}`}
                      >
                        {model.name || model.id}
                      </button>
                    ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
