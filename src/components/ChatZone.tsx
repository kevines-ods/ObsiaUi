/**
 * Zone de conversation (centre).
 *
 * - Branche sur `chat_stream` via `useLlmStream` (événements llm:*).
 * - Sélection fournisseur/modèle héritée de `AppContext`.
 * - Rendu des messages, streaming en cours, erreur et bouton d'arrêt.
 */
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";

import { useApp } from "../context/AppContext";
import { useLlmStream } from "../hooks/useLlmStream";
import type { ChatMessage } from "../types/ipc";

const SYSTEM_PROMPT =
  "Tu es un assistant intégré à un coffre Obsidian. Réponds de façon claire et concise.";

export default function ChatZone(): React.JSX.Element {
  const { selectedProviderId, selectedModel, agents, selectedAgent } = useApp();
  const { tokens, isStreaming, isDone, error, send, stop } = useLlmStream();

  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [pendingAt, setPendingAt] = useState<number | null>(null);

  const bottomRef = useRef<HTMLDivElement | null>(null);
  const commitRef = useRef(false);

  const canSend = input.trim().length > 0 && Boolean(selectedModel) && !isStreaming;

  // Auto-scroll sur nouveaux messages / tokens.
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, tokens, isStreaming]);

  // Commit de la réponse assistant une fois le stream terminé.
  useEffect(() => {
    if (!isDone || pendingAt === null) return;
    if (commitRef.current) return;
    commitRef.current = true;
    const content = tokens;
    setMessages((prev) => {
      const copy = [...prev];
      copy[pendingAt] = { role: "assistant", content };
      return copy;
    });
    setPendingAt(null);
  }, [isDone, pendingAt, tokens]);

  const handleSend = useCallback(async (): Promise<void> => {
    const text = input.trim();
    if (!text || !selectedModel || isStreaming) return;

    const activeAgent = agents.find((a) => a.name === selectedAgent);
    const systemPrompt = activeAgent
      ? `Tu agis selon l'agent « ${activeAgent.name} ».\n${activeAgent.description}`.trim()
      : SYSTEM_PROMPT;

    const userMsg: ChatMessage = { role: "user", content: text };
    const next: ChatMessage[] = [
      { role: "system", content: systemPrompt },
      ...messages,
      userMsg,
    ];
    setMessages(next);
    setInput("");
    // L'emplacement de la réponse assistant est à la suite du message utilisateur.
    setPendingAt(next.length);
    commitRef.current = false;

    await send(next, { provider: selectedProviderId, model: selectedModel });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [input, messages, selectedProviderId, selectedModel, isStreaming, agents, selectedAgent, send]);

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>): void => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void handleSend();
    }
  };

  const onStop = (): void => {
    // Best-effort : ignore les événements restants côté UI.
    stop();
  };

  // Messages historiques (inclut le pending assistant une fois commit).
  const history = pendingAt === null ? messages : messages.slice(0, pendingAt);
  const streamingContent = isStreaming || !isDone ? tokens : null;
  const activeAgent = agents.find((a) => a.name === selectedAgent);

  return (
    <div className="chat-zone">
      {activeAgent && (
        <div className="chat-agent-bar">
          <span className="chat-agent-name">🤖 {activeAgent.name}</span>
          {activeAgent.readOnly && (
            <span className="badge badge-err">lecture seule</span>
          )}
        </div>
      )}
      <div className="messages" aria-live="polite">
        {history.length === 0 && (
          <div className="empty-state">
            <p>Commencez une conversation.</p>
            <p className="empty-hint">
              Choisissez un fournisseur et un modèle dans le sélecteur, puis
              écrivez votre premier message.
            </p>
          </div>
        )}

        {history.map((msg, i) => (
          <div className={`msg ${msg.role}`} key={i}>
            <span className="msg-role">{msg.role}</span>
            <div className="msg-content">{msg.content}</div>
          </div>
        ))}

        {streamingContent !== null && streamingContent.length > 0 && (
          <div className="msg assistant streaming">
            <span className="msg-role">assistant</span>
            <div className="msg-content">
              {streamingContent}
              <span className="stream-caret" aria-hidden="true" />
            </div>
          </div>
        )}

        {error && (
          <div className="error-banner" role="alert">
            ⚠️ {error}
          </div>
        )}

        <div ref={bottomRef} />
      </div>

      <div className="input-row">
        <input
          type="text"
          value={input}
          placeholder={selectedModel ? "Votre message…" : "Sélectionnez un modèle"}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={onKeyDown}
          disabled={!selectedModel}
          aria-label="Message"
        />
        {isStreaming ? (
          <button type="button" className="btn btn-ghost" onClick={onStop}>
            ⏹ Stop
          </button>
        ) : (
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => void handleSend()}
            disabled={!canSend}
          >
            Envoyer
          </button>
        )}
      </div>
    </div>
  );
}
