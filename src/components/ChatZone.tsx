/**
 * Zone de conversation (centre) — rendu de la session active.
 *
 * L'historique et le prompt système sont tenus par le backend : cette zone
 * n'assemble plus de messages, elle envoie du texte et affiche ce qui revient.
 * Le prompt de l'agent est relu dans le coffre à chaque tour, donc modifier un
 * agent prend effet sans rouvrir la session.
 */
import { useEffect, useRef, useState, type KeyboardEvent } from "react";

import { useApp } from "../context/AppContext";
import { useSessions } from "../context/SessionsContext";
import PluginSlot from "./PluginSlot";
import SessionTabs from "./SessionTabs";

export default function ChatZone(): React.JSX.Element {
  const { agents, selectedModel } = useApp();
  const {
    active,
    activeId,
    sessions,
    streaming,
    busy,
    errors,
    speaking,
    teams,
    loading,
    createSession,
    send,
    cancel,
    exportSession,
  } = useSessions();

  const [input, setInput] = useState("");
  const [exportInfo, setExportInfo] = useState<string | null>(null);
  const bottomRef = useRef<HTMLDivElement | null>(null);

  const enCours = activeId ? (busy[activeId] ?? false) : false;
  const partiel = activeId ? streaming[activeId] : undefined;
  const erreur = activeId ? errors[activeId] : errors.__global;
  const agent = agents.find((a) => a.name === active?.agent);
  const equipe = teams.find((t) => t.id === active?.team);
  // En session d'équipe, l'orateur change en cours d'exécution.
  const orateur = activeId ? speaking[activeId] : null;

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [active?.messages.length, partiel]);

  const envoyer = (): void => {
    if (!input.trim() || enCours) return;
    void send(input);
    setInput("");
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>): void => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      envoyer();
    }
  };

  const exporter = async (): Promise<void> => {
    if (!activeId) return;
    const projet = window.prompt("Nom du projet dans la mémoire du coffre :");
    if (!projet?.trim()) return;
    try {
      const entry = await exportSession(activeId, projet);
      setExportInfo(`Exporté dans ${entry.path}`);
    } catch (e) {
      setExportInfo(e instanceof Error ? e.message : String(e));
    }
  };

  // Aucune session ouverte : on invite à en créer une plutôt que d'afficher
  // une conversation vide qui n'accepterait rien.
  if (!loading && sessions.length === 0) {
    return (
      <div className="chat-zone">
        <SessionTabs />
        <div className="empty-state">
          <p>Aucune session ouverte.</p>
          <p className="empty-hint">
            {selectedModel
              ? "Ouvrez une session pour commencer une conversation."
              : "Choisissez d'abord un fournisseur et un modèle dans le sélecteur."}
          </p>
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => void createSession()}
            disabled={!selectedModel}
          >
            Nouvelle session
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="chat-zone">
      <SessionTabs />

      {active && (
        <div className="chat-agent-bar">
          <span className="chat-agent-name">
            {equipe ? `👥 ${equipe.name}` : active.agent ? `🤖 ${active.agent}` : "Sans agent"}
          </span>
          {orateur ? (
            <span className="chat-speaker">au tour de {orateur}</span>
          ) : (
            <span className="chat-model">{equipe ? `${equipe.members.length} membres` : active.model}</span>
          )}
          {agent?.readOnly && <span className="badge badge-err">lecture seule</span>}
          <button
            type="button"
            className="btn btn-mini"
            onClick={() => void exporter()}
            disabled={!active.messages.length}
            title="Écrire la session dans brouillon/ du coffre"
          >
            Exporter
          </button>
          <PluginSlot point="chat-toolbar" />
        </div>
      )}

      <div className="messages" aria-live="polite">
        {active?.messages.map((msg, i) => (
          <div className={`msg ${msg.role}`} key={`${msg.at}-${i}`}>
            <span className="msg-role">{msg.agent ?? msg.role}</span>
            <div className="msg-content">{msg.content}</div>
          </div>
        ))}

        {partiel !== undefined && (
          <div className="msg assistant streaming">
            <span className="msg-role">{orateur ?? active?.agent ?? "assistant"}</span>
            <div className="msg-content">
              {partiel}
              <span className="stream-caret" aria-hidden="true" />
            </div>
          </div>
        )}

        {erreur && (
          <div className="error-banner" role="alert">
            ⚠️ {erreur}
          </div>
        )}
        {exportInfo && <p className="empty-hint">{exportInfo}</p>}

        <div ref={bottomRef} />
      </div>

      <div className="input-row">
        <input
          type="text"
          value={input}
          placeholder={
            !activeId
              ? "Ouvrez une session"
              : equipe
                ? "Objectif confié à l'équipe…"
                : "Votre message…"
          }
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={onKeyDown}
          disabled={!activeId || enCours}
          aria-label="Message"
        />
        {enCours ? (
          <button
            type="button"
            className="btn btn-ghost"
            onClick={() => void cancel(activeId!)}
          >
            ⏹ Stop
          </button>
        ) : (
          <button
            type="button"
            className="btn btn-primary"
            onClick={envoyer}
            disabled={!activeId || !input.trim()}
          >
            Envoyer
          </button>
        )}
      </div>
    </div>
  );
}
