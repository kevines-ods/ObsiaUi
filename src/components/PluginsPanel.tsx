/**
 * Extensions : patches d'interface et plugins.
 *
 * Deux niveaux de pouvoir, présentés comme tels. Un patch décrit — thème,
 * disposition — et n'exécute rien. Un plugin exécute du code dans la fenêtre :
 * il est inactif à l'installation, et toute modification de son fichier le
 * redésactive en attendant une nouvelle approbation.
 */
import { useCallback, useEffect, useState } from "react";

import * as ipc from "../lib/ipc";
import { recharger } from "../lib/plugins";
import type { InstalledPlugin, UiPatch } from "../types/ipc";

/** Pose le CSS des patches actifs sur `:root`. */
export function appliquerCss(css: string): void {
  const id = "obsia-patches";
  let style = document.getElementById(id) as HTMLStyleElement | null;
  if (!style) {
    style = document.createElement("style");
    style.id = id;
    document.head.appendChild(style);
  }
  // Les valeurs sont validées côté backend ; ce qui arrive ici ne contient ni
  // accolade ni point-virgule, donc ne peut pas sortir de la règle.
  style.textContent = css.trim() ? `:root {\n${css}\n}` : "";
}

export default function PluginsPanel(): React.JSX.Element {
  const [patches, setPatches] = useState<UiPatch[]>([]);
  const [plugins, setPlugins] = useState<InstalledPlugin[]>([]);
  const [dossier, setDossier] = useState("");
  const [erreurs, setErreurs] = useState<string[]>([]);
  const [ouvert, setOuvert] = useState(false);

  const rafraichir = useCallback(async (): Promise<void> => {
    try {
      const [p, pl, dir, css] = await Promise.all([
        ipc.patchesList(),
        ipc.pluginsList(),
        ipc.pluginsDir(),
        ipc.patchCss(),
      ]);
      setPatches(p);
      setPlugins(pl);
      setDossier(dir);
      appliquerCss(css);
      const { erreurs: e } = await recharger();
      setErreurs(e);
    } catch (e) {
      setErreurs([e instanceof Error ? e.message : String(e)]);
    }
  }, []);

  useEffect(() => {
    void rafraichir();
  }, [rafraichir]);

  const basculerPatch = async (p: UiPatch): Promise<void> => {
    try {
      appliquerCss(await ipc.patchToggle(p.id, !p.enabled));
      setPatches((prev) =>
        prev.map((x) => (x.id === p.id ? { ...x, enabled: !p.enabled } : x)),
      );
    } catch (e) {
      setErreurs([e instanceof Error ? e.message : String(e)]);
    }
  };

  const basculerPlugin = async (p: InstalledPlugin): Promise<void> => {
    try {
      if (p.enabled) await ipc.pluginDisable(p.id);
      else await ipc.pluginEnable(p.id);
      await rafraichir();
    } catch (e) {
      setErreurs([e instanceof Error ? e.message : String(e)]);
    }
  };

  return (
    <section className="panel-section">
      <div className="dropdown-head">
        <span>Extensions</span>
        <button type="button" className="link" onClick={() => setOuvert((v) => !v)}>
          {ouvert ? "Fermer" : "Gérer"}
        </button>
      </div>

      {patches.map((p) => (
        <div className="team-row" key={p.id}>
          <div className="team-body">
            <div className="team-title">{p.name}</div>
            <div className="runtime-meta">
              patch · {Object.keys(p.theme).length} jeton(s)
              {p.description && ` · ${p.description}`}
            </div>
          </div>
          <button
            type="button"
            className={`btn btn-mini ${p.enabled ? "btn-primary" : ""}`}
            onClick={() => void basculerPatch(p)}
          >
            {p.enabled ? "Actif" : "Appliquer"}
          </button>
          {ouvert && (
            <button
              type="button"
              className="session-close"
              aria-label={`Supprimer ${p.name}`}
              onClick={() => void ipc.patchDelete(p.id).then(rafraichir)}
            >
              ×
            </button>
          )}
        </div>
      ))}

      {plugins.map((p) => (
        <div className="team-row" key={p.id}>
          <div className="team-body">
            <div className="team-title">
              {p.name} <span className="runtime-version">v{p.version}</span>
            </div>
            <div className="runtime-meta">
              plugin · {p.permissions.join(", ") || "aucune permission"}
            </div>
            {p.needsReview && (
              <div className="runtime-meta err-text">
                le fichier a changé depuis l'approbation — à relire avant
                réactivation
              </div>
            )}
          </div>
          <button
            type="button"
            className={`btn btn-mini ${p.enabled ? "btn-primary" : ""}`}
            onClick={() => void basculerPlugin(p)}
            title={
              p.enabled
                ? "Désactiver"
                : "Approuver le code présent et activer"
            }
          >
            {p.enabled ? "Actif" : p.needsReview ? "Réapprouver" : "Activer"}
          </button>
        </div>
      ))}

      {patches.length === 0 && plugins.length === 0 && (
        <p className="empty-hint">
          Aucune extension. Un patch retouche le thème et la disposition sans
          exécuter de code ; un plugin ajoute des fonctions.
        </p>
      )}

      {ouvert && (
        <div className="team-form">
          <p className="empty-hint">
            Déposez un plugin dans <code>{dossier}</code> (un dossier par
            plugin, avec <code>plugin.json</code> et son fichier
            <code> .js</code>), puis rafraîchissez.
          </p>
          <p className="empty-hint">
            Un plugin s'exécute dans cette fenêtre : ses permissions bornent
            l'API qu'ObsiaUi lui tend, pas ce que son code peut atteindre.
            N'activez que ce que vous avez lu.
          </p>
          <div className="team-actions">
            <button type="button" className="btn btn-mini" onClick={() => void rafraichir()}>
              Rafraîchir
            </button>
          </div>
        </div>
      )}

      {erreurs.map((e) => (
        <p className="err-text" key={e}>
          {e}
        </p>
      ))}
    </section>
  );
}
