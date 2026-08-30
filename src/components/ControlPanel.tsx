/**
 * Panneau de configuration (gauche).
 *
 * - `config_get` / `config_set` : clés API, chemin du coffre (`vault_path`),
 *   `ollamaHost` et fournisseur par défaut.
 * - `models_list` : liste des modèles d'un fournisseur donné.
 * - `provider_test` : test de santé (hotel via AppContext).
 */
import { useEffect, useState } from "react";

import { useApp } from "../context/AppContext";
import PlansPanel from "./PlansPanel";
import RemotePanel from "./RemotePanel";
import TeamsPanel from "./TeamsPanel";
import * as ipc from "../lib/ipc";
import type { ConfigView, ModelInfo } from "../types/ipc";

type FormState = Pick<ConfigView, "defaultProvider" | "ollamaHost" | "vaultPath">;

export default function ControlPanel(): React.JSX.Element {
  const {
    config,
    providers,
    health,
    testingId,
    testProvider,
    saveConfig,
    loadError,
    selectedProviderId,
  } = useApp();

  const [form, setForm] = useState<FormState>({
    defaultProvider: "",
    ollamaHost: "",
    vaultPath: "",
  });
  const [apiKeys, setApiKeys] = useState<Record<string, string>>({});
  const [savingConfig, setSavingConfig] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [loadingModels, setLoadingModels] = useState(false);

  // Hydratation du formulaire depuis config_get (via AppContext).
  useEffect(() => {
    if (!config) return;
    setForm((prev) => ({
      defaultProvider: config.defaultProvider ?? prev.defaultProvider,
      ollamaHost: config.ollamaHost ?? prev.ollamaHost,
      vaultPath: config.vaultPath ?? prev.vaultPath,
    }));
  }, [config]);

  const configuredKeys = config?.apiKeysConfigured ?? [];

  // Chargement des modèles du fournisseur sélectionné.
  useEffect(() => {
    if (!selectedProviderId) {
      setModels([]);
      return;
    }
    let cancelled = false;
    setLoadingModels(true);
    void ipc
      .modelsList(selectedProviderId)
      .then((m) => {
        if (!cancelled) setModels(m);
      })
      .catch(() => {
        if (!cancelled) setModels([]);
      })
      .finally(() => {
        if (!cancelled) setLoadingModels(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedProviderId]);

  const updateForm = (patch: Partial<FormState>): void =>
    setForm((prev) => ({ ...prev, ...patch }));

  const handleSaveConfig = async (): Promise<void> => {
    setSavingConfig(true);
    setNotice(null);
    const cfg = await saveConfig({
      defaultProvider: form.defaultProvider || undefined,
      ollamaHost: form.ollamaHost || undefined,
      vaultPath: form.vaultPath || undefined,
    });
    setSavingConfig(false);
    if (cfg) {
      setNotice("Configuration mise à jour.");
      setApiKeys({});
    }
  };

  const handleSetKey = async (providerId: string): Promise<void> => {
    const apiKey = apiKeys[providerId]?.trim();
    if (!apiKey) return;
    setSavingConfig(true);
    setNotice(null);
    const cfg = await saveConfig({ setApiKey: { providerId, apiKey } });
    setSavingConfig(false);
    if (cfg) {
      setNotice(`Clé enregistrée pour ${providerId}.`);
      setApiKeys((prev) => ({ ...prev, [providerId]: "" }));
    }
  };

  return (
    <div className="control-panel">
      <h2 className="panel-title">Configuration</h2>

      <TeamsPanel />

      <PlansPanel />

      <RemotePanel />

      {loadError && <p className="err-text">{loadError}</p>}

      {/* Fournisseurs + test de santé */}
      <section className="cp-section">
        <h3 className="cp-section-title">Fournisseurs</h3>
        {providers.length === 0 && <p className="empty-hint">Aucun fournisseur.</p>}
        {providers.map((provider) => {
          const h = health[provider.id];
          return (
            <div className="cp-row" key={provider.id}>
              <span className="cp-label">{provider.name}</span>
              <button
                type="button"
                className="btn btn-mini"
                onClick={() => void testProvider(provider.id)}
                disabled={testingId === provider.id}
              >
                {testingId === provider.id ? "…" : "Tester"}
              </button>
              {h && <span className={`badge ${h.ok ? "badge-ok" : "badge-err"}`}>{h.ok ? "OK" : h.error ?? "Erreur"}</span>}
            </div>
          );
        })}
      </section>

      {/* Configuration générale */}
      <section className="cp-section">
        <h3 className="cp-section-title">Général</h3>

        <label className="field">
          <span className="label">Fournisseur par défaut</span>
          <select
            value={form.defaultProvider ?? ""}
            onChange={(e) => updateForm({ defaultProvider: e.target.value })}
          >
            <option value="">— Aucun —</option>
            {providers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        </label>

        <label className="field">
          <span className="label">Hôte Ollama</span>
          <input
            type="text"
            value={form.ollamaHost ?? ""}
            placeholder="http://localhost:11434"
            onChange={(e) => updateForm({ ollamaHost: e.target.value })}
          />
        </label>

        <label className="field">
          <span className="label">Chemin du coffre (vault_path)</span>
          <input
            type="text"
            value={form.vaultPath ?? ""}
            placeholder="~/Mon coffre"
            onChange={(e) => updateForm({ vaultPath: e.target.value })}
          />
        </label>

        <button
          type="button"
          className="btn btn-primary"
          onClick={() => void handleSaveConfig()}
          disabled={savingConfig}
        >
          {savingConfig ? "…" : "Sauvegarder la config"}
        </button>
        {notice && <p className="notice">{notice}</p>}
      </section>

      {/* Clés API */}
      <section className="cp-section">
        <h3 className="cp-section-title">Clés API</h3>
        {providers.length === 0 && <p className="empty-hint">Aucun fournisseur.</p>}
        {providers.map((provider) => {
          const hasKey = configuredKeys.includes(provider.id);
          return (
            <div className="cp-row key-row" key={provider.id}>
              <span className="cp-label">{provider.name}</span>
              {hasKey ? (
                <span className="badge badge-ok">clé configurée</span>
              ) : (
                <span className="badge">pas de clé</span>
              )}
              <input
                type="password"
                placeholder="Clé API…"
                value={apiKeys[provider.id] ?? ""}
                onChange={(e) =>
                  setApiKeys((prev) => ({ ...prev, [provider.id]: e.target.value }))
                }
                aria-label={`Clé API ${provider.name}`}
              />
              <button
                type="button"
                className="btn btn-mini"
                onClick={() => void handleSetKey(provider.id)}
                disabled={!apiKeys[provider.id]?.trim() || savingConfig}
              >
                Enregistrer
              </button>
            </div>
          );
        })}
      </section>

      {/* Modèles du fournisseur sélectionné */}
      <section className="cp-section">
        <h3 className="cp-section-title">Modèles — {selectedProviderId || "aucun"}</h3>
        {loadingModels && <p className="empty-hint">Chargement…</p>}
        {!loadingModels && models.length === 0 && (
          <p className="empty-hint">Aucun modèle.</p>
        )}
        <ul className="cp-models">
          {models.map((model) => (
            <li key={model.id} className="cp-row">
              <span className="cp-label">{model.name || model.id}</span>
              <span className="cp-meta">
                {model.context_window ? `ctx ${model.context_window} · ` : ""}
                {(model.capabilities ?? []).join(", ")}
              </span>
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}
