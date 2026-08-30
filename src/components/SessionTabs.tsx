/**
 * Barre d'onglets des sessions.
 *
 * Une pastille signale les sessions qui répondent — y compris celles qui ne
 * sont pas à l'écran : c'est tout l'intérêt d'avoir plusieurs sessions, on
 * doit voir qu'elles travaillent sans avoir à les ouvrir.
 */
import { useState } from "react";

import { useSessions } from "../context/SessionsContext";

export default function SessionTabs(): React.JSX.Element {
  const {
    sessions,
    activeId,
    busy,
    loading,
    createSession,
    selectSession,
    renameSession,
    deleteSession,
  } = useSessions();

  const [renommage, setRenommage] = useState<string | null>(null);
  const [brouillon, setBrouillon] = useState("");

  const commencerRenommage = (id: string, titre: string): void => {
    setRenommage(id);
    setBrouillon(titre);
  };

  const validerRenommage = async (): Promise<void> => {
    const id = renommage;
    setRenommage(null);
    if (id && brouillon.trim()) {
      await renameSession(id, brouillon);
    }
  };

  return (
    <div className="session-tabs" role="tablist" aria-label="Sessions">
      {sessions.map((s) => {
        const actif = s.id === activeId;
        return (
          <div
            key={s.id}
            role="tab"
            aria-selected={actif}
            tabIndex={0}
            className={`session-tab ${actif ? "active" : ""}`}
            onClick={() => selectSession(s.id)}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                selectSession(s.id);
              }
            }}
            onDoubleClick={() => commencerRenommage(s.id, s.title)}
            title={`${s.model}${s.agent ? ` — ${s.agent}` : ""}`}
          >
            {busy[s.id] && <span className="session-dot" aria-label="en cours" />}
            {renommage === s.id ? (
              <input
                className="session-rename"
                value={brouillon}
                autoFocus
                onChange={(e) => setBrouillon(e.target.value)}
                onBlur={() => void validerRenommage()}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void validerRenommage();
                  if (e.key === "Escape") setRenommage(null);
                }}
                onClick={(e) => e.stopPropagation()}
                aria-label="Renommer la session"
              />
            ) : (
              <span className="session-title">{s.title}</span>
            )}
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
        );
      })}

      <button
        type="button"
        className="session-new"
        onClick={() => void createSession()}
        disabled={loading}
        aria-label="Nouvelle session"
        title="Nouvelle session"
      >
        +
      </button>
    </div>
  );
}
