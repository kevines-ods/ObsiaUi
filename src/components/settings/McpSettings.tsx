/**
 * Outils MCP déclarés dans le coffre.
 *
 * ObsiaUi ne se connecte pas aux serveurs MCP : le coffre les déclare, le
 * harness les branche. Cet écran sert donc à voir ce qui est déclaré, qui
 * l'utilise, et à rédiger une nouvelle déclaration.
 *
 * La rédaction va dans `brouillon/`, jamais dans `IA/MCP/`. Donner à des
 * agents un outil que personne n'a relu reviendrait à leur ouvrir un accès
 * sans revue.
 */
import { useEffect, useState } from "react";

import * as ipc from "../../lib/ipc";
import type { McpInfo } from "../../types/ipc";

export default function McpSettings(): React.JSX.Element {
  const [outils, setOutils] = useState<McpInfo[]>([]);
  const [contenu, setContenu] = useState<Record<string, string>>({});
  const [ouvert, setOuvert] = useState<string | null>(null);
  const [erreur, setErreur] = useState<string | null>(null);

  const [redaction, setRedaction] = useState(false);
  const [nom, setNom] = useState("");
  const [description, setDescription] = useState("");
  const [corps, setCorps] = useState("");
  const [notice, setNotice] = useState<string | null>(null);

  const recharger = async (): Promise<void> => {
    try {
      setOutils(await ipc.mcpList());
      setErreur(null);
    } catch (e) {
      setErreur(e instanceof Error ? e.message : String(e));
    }
  };

  useEffect(() => {
    void recharger();
  }, []);

  const afficher = async (outil: McpInfo): Promise<void> => {
    if (ouvert === outil.path) {
      setOuvert(null);
      return;
    }
    setOuvert(outil.path);
    if (contenu[outil.path] !== undefined) return;
    try {
      const texte = await ipc.vaultRead(outil.path);
      setContenu((prev) => ({ ...prev, [outil.path]: texte }));
    } catch (e) {
      setContenu((prev) => ({
        ...prev,
        [outil.path]: e instanceof Error ? e.message : String(e),
      }));
    }
  };

  const rediger = async (): Promise<void> => {
    try {
      const chemin = await ipc.mcpDraft(nom, description, corps);
      setNotice(`Écrit dans ${chemin} — à relire puis déplacer par patch.`);
      setNom("");
      setDescription("");
      setCorps("");
      setRedaction(false);
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="settings-body">
      {erreur && <p className="err-text">{erreur}</p>}

      {outils.length === 0 && !erreur && (
        <p className="empty-hint">
          Aucun outil déclaré dans <code>IA/MCP/</code> du coffre.
        </p>
      )}

      {outils.map((o) => (
        <section className="provider-block" key={o.path}>
          <div className="cp-row">
            <span className="cp-label">{o.name}</span>
            <button type="button" className="btn btn-mini" onClick={() => void afficher(o)}>
              {ouvert === o.path ? "Masquer" : "Voir"}
            </button>
          </div>
          {o.description && <p className="runtime-meta">{o.description}</p>}
          <p className="runtime-meta">
            {o.declaredBy.length === 0
              ? "déclaré par aucun agent"
              : `utilisé par ${o.declaredBy.join(", ")}`}
          </p>
          {ouvert === o.path && (
            <pre className="mcp-source">{contenu[o.path] ?? "…"}</pre>
          )}
        </section>
      ))}

      <div className="team-actions">
        <button
          type="button"
          className="btn btn-mini"
          onClick={() => setRedaction((v) => !v)}
        >
          {redaction ? "Annuler" : "Déclarer un outil"}
        </button>
        <button type="button" className="btn btn-mini" onClick={() => void recharger()}>
          Rafraîchir
        </button>
      </div>

      {redaction && (
        <div className="team-form">
          <label>
            Nom
            <input value={nom} placeholder="git-hub" onChange={(e) => setNom(e.target.value)} />
          </label>
          <label>
            Description
            <input
              value={description}
              placeholder="Une ligne — quoi et quand."
              onChange={(e) => setDescription(e.target.value)}
            />
          </label>
          <label>
            Contenu
            <textarea
              rows={6}
              value={corps}
              placeholder="Outils exposés, permissions, sécurité…"
              onChange={(e) => setCorps(e.target.value)}
            />
          </label>
          <div className="team-actions">
            <button
              type="button"
              className="btn btn-primary btn-mini"
              onClick={() => void rediger()}
              disabled={!nom.trim()}
            >
              Écrire dans brouillon/
            </button>
          </div>
        </div>
      )}

      {notice && <p className="runtime-meta">{notice}</p>}

      <p className="empty-hint">
        ObsiaUi n'appelle pas ces outils : le coffre les déclare, le harness qui
        le charge les branche. La configuration à recopier est produite par
        <code> generer_prompt.py --mcp</code>.
      </p>
    </div>
  );
}
