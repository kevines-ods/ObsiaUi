/**
 * Liste des sessions, dans le panneau latéral.
 *
 * Les sessions étaient des onglets en haut du chat. Au-delà de trois ou
 * quatre, les titres se tronquaient jusqu'à devenir indistinguables et la
 * barre débordait. Une liste verticale tient la longueur : elle défile, les
 * titres restent lisibles, et chaque ligne peut porter son agent et son
 * modèle.
 *
 * La pastille d'activité reste visible sur les sessions qui répondent, y
 * compris celles qu'on ne regarde pas — c'est tout l'intérêt d'en avoir
 * plusieurs.
 */
import { useState } from "react";

import { useSessions } from "../context/SessionsContext";

export default function SessionsPanel(): React.JSX.Element {
  const {
    sessions,
    activeId,
    busy,
    teams,
    selectSession,
    renameSession,
    deleteSession,
  } = useSessions();

  const [renommage, setRenommage] = useState<string | null>(null);
  const [brouillon, setBrouillon] = useState("");

  const valider = async (): Promise<void> => {
    const id = renommage;
    setRenommage(null);
    if (id && brouillon.trim()) await renameSession(id, brouillon);
  };

  if (sessions.length === 0) {
    return (
      <p className="empty-hint">
        Aucune session. Le bouton « + » en ouvre une avec l'agent et le modèle
        sélectionnés en haut.
      </p>
    );
  }

  return (
    <ul className="session-list">
      {sessions.map((s) => {
        const actif = s.id === activeId;
        const equipe = teams.find((t) => t.id === s.team);
        return (
          <li key={s.id}>
            <div
              className={`session-item ${actif ? "active" : ""}`}
              role="button"
              tabIndex={0}
              aria-current={actif}
              onClick={() => selectSession(s.id)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  selectSession(s.id);
                }
              }}
              onDoubleClick={() => {
                setRenommage(s.id);
                setBrouillon(s.title);
              }}
            >
              {busy[s.id] && <span className="session-dot" aria-label="en cours" />}
              <div className="session-item-body">
                {renommage === s.id ? (
                  <input
                    className="session-rename"
                    value={brouillon}
                    autoFocus
                    onChange={(e) => setBrouillon(e.target.value)}
                    onBlur={() => void valider()}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") void valider();
                      if (e.key === "Escape") setRenommage(null);
                    }}
                    onClick={(e) => e.stopPropagation()}
                    aria-label="Renommer la session"
                  />
                ) : (
                  <span className="session-name">{s.title}</span>
                )}
                <span className="session-meta">
                  {equipe ? `👥 ${equipe.name}` : (s.agent ?? "sans agent")} · {s.model}
                </span>
              </div>
              <button
                type="button"
                className="session-close"
                aria-label={`Fermer ${s.title}`}
                onClick={(e) => {
                  e.stopPropagation();
                  void deleteSession(s.id);
                }}
              >
                ×
              </button>
            </div>
          </li>
        );
      })}
    </ul>
  );
}
