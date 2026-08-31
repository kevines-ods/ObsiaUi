/**
 * Sessions à distance.
 *
 * Deux moitiés indépendantes, et c'est voulu : une machine peut n'être
 * qu'hôte, ou n'être que client, ou les deux.
 *
 * - **Cette machine** — le daemon qui expose ce harness. Le jeton est affiché
 *   à la demande, pour être recopié sur le poste client.
 * - **S'attacher à un hôte** — bascule tous les appels et tous les flux vers
 *   une autre instance. Le coffre, les modèles et les sessions sont alors
 *   ceux de l'hôte.
 */
import { useEffect, useState } from "react";

import * as ipc from "../lib/ipc";
import {
  cibleCourante,
  connecter,
  normaliserUrl,
  surChangementDeCible,
  verifierHote,
  type Cible,
} from "../lib/transport";
import type { RemoteStatus } from "../types/ipc";

export default function RemotePanel(): React.JSX.Element {
  const [statut, setStatut] = useState<RemoteStatus | null>(null);
  const [jeton, setJeton] = useState<string | null>(null);
  const [cible, setCible] = useState<Cible>(cibleCourante());
  const [hote, setHote] = useState("");
  const [jetonClient, setJetonClient] = useState("");
  const [occupe, setOccupe] = useState(false);
  const [erreur, setErreur] = useState<string | null>(null);
  const [ouvert, setOuvert] = useState(false);

  const rafraichir = async (): Promise<void> => {
    try {
      setStatut(await ipc.remoteStatus());
    } catch (e) {
      setErreur(e instanceof Error ? e.message : String(e));
    }
  };

  useEffect(() => {
    void rafraichir();
    return surChangementDeCible(setCible);
  }, []);

  const agir = async (action: () => Promise<unknown>): Promise<void> => {
    setOccupe(true);
    setErreur(null);
    try {
      await action();
    } catch (e) {
      setErreur(e instanceof Error ? e.message : String(e));
    } finally {
      setOccupe(false);
      await rafraichir();
    }
  };

  const attacher = async (): Promise<void> => {
    const url = normaliserUrl(hote);
    if (!url || !jetonClient.trim()) return;
    setOccupe(true);
    setErreur(null);
    try {
      // On vérifie que l'hôte répond avant de basculer : sinon l'interface se
      // retrouverait attachée à une adresse morte, sans plus rien afficher.
      if (!(await verifierHote(url))) {
        setErreur("aucune instance ObsiaUi ne répond à cette adresse");
        return;
      }
      await connecter({ kind: "remote", url, token: jetonClient.trim() });
    } catch (e) {
      setErreur(e instanceof Error ? e.message : String(e));
    } finally {
      setOccupe(false);
    }
  };

  const detacher = async (): Promise<void> => {
    await connecter({ kind: "local" });
  };

  return (
    <div className="panel-block">
      <div className="panel-actions"><button type="button" className="link" onClick={() => setOuvert((v) => !v)}>
          {ouvert ? "Fermer" : "Configurer"}
        </button></div>

      <div className="runtime-row">
        <span className={`badge ${cible.kind === "remote" ? "badge-ok" : ""}`}>
          {cible.kind === "remote" ? "attaché" : "local"}
        </span>
        <div className="runtime-body">
          <div className="runtime-meta">
            {cible.kind === "remote"
              ? `Harness de ${cible.url}`
              : "Harness de cette machine"}
          </div>
          {statut?.running && (
            <div className="runtime-meta">
              Serveur en écoute sur <code>{statut.address}</code>
              {!statut.loopbackOnly && " — ouvert au réseau"}
            </div>
          )}
        </div>
        {cible.kind === "remote" && (
          <button type="button" className="btn btn-mini" onClick={() => void detacher()}>
            Détacher
          </button>
        )}
      </div>

      {ouvert && (
        <div className="team-form">
          <div className="dropdown-head">
            <span>Cette machine</span>
          </div>
          <div className="team-actions">
            {statut?.running ? (
              <button
                type="button"
                className="btn btn-mini"
                disabled={occupe}
                onClick={() => void agir(ipc.remoteStop)}
              >
                Arrêter le serveur
              </button>
            ) : (
              <button
                type="button"
                className="btn btn-mini"
                disabled={occupe}
                onClick={() => void agir(ipc.remoteStart)}
              >
                Démarrer le serveur
              </button>
            )}
            <button
              type="button"
              className="btn btn-mini"
              disabled={occupe || !statut?.tokenConfigured}
              onClick={() =>
                void agir(async () => setJeton(await ipc.remoteTokenRead()))
              }
            >
              Voir le jeton
            </button>
            <button
              type="button"
              className="btn btn-mini"
              disabled={occupe}
              onClick={() =>
                void agir(async () => setJeton(await ipc.remoteTokenRotate()))
              }
              title="Invalide l'ancien jeton immédiatement"
            >
              Renouveler
            </button>
          </div>
          {jeton && (
            <p className="runtime-meta">
              Jeton : <code>{jeton}</code>
            </p>
          )}
          <p className="empty-hint">
            L'écoute par défaut est <code>127.0.0.1</code>. Pour ouvrir au
            réseau, renseignez l'adresse d'écoute dans la configuration —
            le jeton reste exigé dans tous les cas.
          </p>

          <div className="dropdown-head">
            <span>S'attacher à un hôte</span>
          </div>
          <label>
            Adresse
            <input
              value={hote}
              placeholder="gpu.lan:7420"
              onChange={(e) => setHote(e.target.value)}
            />
          </label>
          <label>
            Jeton de l'hôte
            <input
              type="password"
              value={jetonClient}
              onChange={(e) => setJetonClient(e.target.value)}
            />
          </label>
          <div className="team-actions">
            <button
              type="button"
              className="btn btn-primary btn-mini"
              disabled={occupe || !hote.trim() || !jetonClient.trim()}
              onClick={() => void attacher()}
            >
              {occupe ? "…" : "S'attacher"}
            </button>
          </div>
          {erreur && <p className="err-text">{erreur}</p>}
        </div>
      )}
      {!ouvert && erreur && <p className="err-text">{erreur}</p>}
    </div>
  );
}
