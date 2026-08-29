/**
 * Sélecteur d'agent (zone dédiée, topbar).
 *
 * - Liste les agents du coffre `IA/agents/*.md` via `vault_list` + `vault_read`
 *   (les agents sont des notes Markdown avec frontmatter).
 * - Affiche la description, les skills et le statut lecture seule.
 * - La sélection est partagée via `AppContext` (utilisée notamment comme
 *   contexte système de la conversation).
 */
import { useEffect, useState } from "react";

import { useApp } from "../context/AppContext";

export default function AgentSelector(): React.JSX.Element {
  const {
    agents,
    loadingAgents,
    selectedAgent,
    selectAgent,
    loadAgents,
  } = useApp();

  const [open, setOpen] = useState(false);

  const current = agents.find((a) => a.name === selectedAgent);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  return (
    <div className="provider-selector agent-selector">
      <button
        type="button"
        className="btn btn-ghost"
        onClick={() => setOpen((v) => !v)}
        disabled={loadingAgents}
        aria-haspopup="listbox"
        aria-expanded={open}
        data-testid="agent-selector-trigger"
      >
        {loadingAgents ? "Chargement…" : `🤖 ${current?.name ?? "Agent"}`}
      </button>

      {open && (
        <div
          className="provider-dropdown agent-dropdown"
          role="listbox"
          aria-label="Agents"
        >
          <div className="dropdown-head">
            <span>Agents (IA/agents)</span>
            <button type="button" className="link" onClick={() => void loadAgents()}>
              Rafraîchir
            </button>
          </div>

          {agents.length === 0 && !loadingAgents && (
            <p className="empty-hint">Aucun agent trouvé dans IA/agents.</p>
          )}

          {agents.map((agent) => (
            <div className="provider-group" key={agent.path}>
              <button
                type="button"
                className={`agent-item ${agent.name === selectedAgent ? "active" : ""}`}
                onClick={() => {
                  selectAgent(agent.name);
                  setOpen(false);
                }}
              >
                <span className="cp-label">{agent.name}</span>
                {agent.readOnly && (
                  <span className="badge badge-err">lecture seule</span>
                )}
              </button>

              {agent.description && (
                <p className="agent-desc">{agent.description}</p>
              )}

              {agent.skills.length > 0 && (
                <div className="model-list">
                  {agent.skills.map((skill) => (
                    <span className="agent-skill" key={skill}>
                      {skill}
                    </span>
                  ))}
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
