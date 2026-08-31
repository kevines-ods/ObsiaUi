/**
 * Zone du coffre : notes ou graphe.
 *
 * Le coffre n'est pas un dossier de fichiers, c'est un réseau de notes liées.
 * L'explorateur seul ne montrait que la liste ; le graphe montre la structure
 * — quelles notes sont des pivots, lesquelles sont isolées, quels liens
 * pointent vers rien.
 */
import { useState } from "react";

import FileManager from "./FileManager";
import VaultGraph from "./VaultGraph";
import * as ipc from "../lib/ipc";

type Vue = "notes" | "graphe";

export default function VaultPanel(): React.JSX.Element {
  const [vue, setVue] = useState<Vue>("notes");
  const [aOuvrir, setAOuvrir] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [plein, setPlein] = useState(false);

  const ouvrirNote = (path: string): void => {
    // Cliquer un nœud amène la note dans l'éditeur plutôt que d'ouvrir une
    // seconde vue : c'est le même coffre.
    setAOuvrir(path);
    setPlein(false);
    setVue("notes");
  };

  const ouvrirDansObsidian = async (): Promise<void> => {
    if (!aOuvrir) return;
    try {
      await ipc.vaultOpenExternal(aOuvrir);
      setNotice(null);
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="vault-panel">
      <div className="vault-tabs" role="tablist" aria-label="Vue du coffre">
        {(["notes", "graphe"] as Vue[]).map((v) => (
          <button
            key={v}
            type="button"
            role="tab"
            aria-selected={vue === v}
            className={`modal-tab ${vue === v ? "active" : ""}`}
            onClick={() => setVue(v)}
          >
            {v === "notes" ? "Notes" : "Graphe"}
          </button>
        ))}
        {aOuvrir && (
          <button
            type="button"
            className="link"
            onClick={() => void ouvrirDansObsidian()}
            title="Ouvrir cette note dans Obsidian"
          >
            Obsidian ↗
          </button>
        )}
      </div>

      {notice && <p className="err-text">{notice}</p>}

      {vue === "notes" ? (
        <FileManager openPath={aOuvrir} />
      ) : (
        <VaultGraph onOpen={ouvrirNote} onPlein={setPlein} />
      )}

      {/* Un panneau latéral reste étroit pour un graphe : le plein écran
          donne la place de lire la structure. */}
      {plein && (
        <div className="modal-backdrop" onClick={() => setPlein(false)} role="presentation">
          <div
            className="modal modal-large"
            role="dialog"
            aria-modal="true"
            aria-label="Graphe du coffre"
            onClick={(e) => e.stopPropagation()}
          >
            <VaultGraph onOpen={ouvrirNote} plein onPlein={setPlein} />
          </div>
        </div>
      )}
    </div>
  );
}
