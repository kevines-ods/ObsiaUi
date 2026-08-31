/**
 * Réglages généraux : thème, coffre, hôtes des moteurs locaux.
 */
import { useEffect, useState } from "react";

import { useApp } from "../../context/AppContext";
import type { ConfigView, Theme } from "../../types/ipc";

type Formulaire = Pick<
  ConfigView,
  "defaultProvider" | "ollamaHost" | "llamacppHost" | "vaultPath"
>;

const VIDE: Formulaire = {
  defaultProvider: "",
  ollamaHost: "",
  llamacppHost: "",
  vaultPath: "",
};

const THEMES: Array<{ valeur: Theme; libelle: string; aide: string }> = [
  { valeur: "dark", libelle: "Sombre", aide: "Défaut de l'application." },
  { valeur: "light", libelle: "Clair", aide: "" },
  { valeur: "system", libelle: "Système", aide: "Suit le réglage du bureau." },
];

export default function GeneralSettings(): React.JSX.Element {
  const { config, providers, saveConfig } = useApp();
  const [form, setForm] = useState<Formulaire>(VIDE);
  const [enregistrement, setEnregistrement] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    if (!config) return;
    setForm({
      defaultProvider: config.defaultProvider ?? "",
      ollamaHost: config.ollamaHost ?? "",
      llamacppHost: config.llamacppHost ?? "",
      vaultPath: config.vaultPath ?? "",
    });
  }, [config]);

  const maj = (patch: Partial<Formulaire>): void =>
    setForm((prev) => ({ ...prev, ...patch }));

  const enregistrer = async (): Promise<void> => {
    setEnregistrement(true);
    setNotice(null);
    const cfg = await saveConfig({
      defaultProvider: form.defaultProvider ?? "",
      ollamaHost: form.ollamaHost ?? "",
      llamacppHost: form.llamacppHost ?? "",
      vaultPath: form.vaultPath ?? "",
    });
    setEnregistrement(false);
    setNotice(cfg ? "Réglages enregistrés." : "Échec de l'enregistrement.");
  };

  // Le thème s'applique immédiatement : attendre un bouton « enregistrer »
  // pour voir la couleur changer n'a aucun sens.
  const choisirTheme = (theme: Theme): void => {
    void saveConfig({ theme });
  };

  return (
    <div className="settings-body">
      <fieldset className="field-group">
        <legend>Thème</legend>
        <div className="theme-choices">
          {THEMES.map((t) => (
            <label key={t.valeur} className="theme-choice">
              <input
                type="radio"
                name="theme"
                checked={(config?.theme ?? "dark") === t.valeur}
                onChange={() => choisirTheme(t.valeur)}
              />
              <span>{t.libelle}</span>
              {t.aide && <small className="runtime-meta">{t.aide}</small>}
            </label>
          ))}
        </div>
      </fieldset>

      <label className="field">
        <span className="label">Fournisseur par défaut</span>
        <select
          value={form.defaultProvider ?? ""}
          onChange={(e) => maj({ defaultProvider: e.target.value })}
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
        <span className="label">Chemin du coffre</span>
        <input
          type="text"
          value={form.vaultPath ?? ""}
          placeholder="~/OBSIA/obsia_vault"
          onChange={(e) => maj({ vaultPath: e.target.value })}
        />
        <small className="runtime-meta">
          Laissé vide, le coffre est cherché à côté de l'application puis dans
          le dossier personnel.
        </small>
      </label>

      <label className="field">
        <span className="label">Hôte Ollama</span>
        <input
          type="text"
          value={form.ollamaHost ?? ""}
          placeholder="détecté automatiquement"
          onChange={(e) => maj({ ollamaHost: e.target.value })}
        />
      </label>

      <label className="field">
        <span className="label">Hôte llama.cpp</span>
        <input
          type="text"
          value={form.llamacppHost ?? ""}
          placeholder="détecté automatiquement"
          onChange={(e) => maj({ llamacppHost: e.target.value })}
        />
        <small className="runtime-meta">
          Ces deux champs ne servent qu'à forcer une adresse : la détection
          trouve seule les moteurs locaux, y compris sur un port inhabituel.
        </small>
      </label>

      <div className="team-actions">
        <button
          type="button"
          className="btn btn-primary btn-mini"
          onClick={() => void enregistrer()}
          disabled={enregistrement}
        >
          {enregistrement ? "…" : "Enregistrer"}
        </button>
        {notice && <span className="runtime-meta">{notice}</span>}
      </div>
    </div>
  );
}
