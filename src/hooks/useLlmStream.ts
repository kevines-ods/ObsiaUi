import { useEffect, useState, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

/** Type miroir du contrat IPC Rust (ChatMessage). */
export interface ChatMessage {
  role: "system" | "user" | "assistant";
  content: string;
  name?: string;
}

/** Options de chat (contrat IPC ChatRequestPayload). */
export interface ChatOptions {
  temperature?: number;
  maxTokens?: number;
  stream?: boolean;
}

/**
 * Hook de streaming LLM — branché sur le contrat IPC Tauri :
 * - commande : `chat_stream` (payload { provider, model, messages, options })
 * - événements : `llm:token` (chunk), `llm:done`, `llm:error`
 */
export function useLlmStream() {
  const [tokens, setTokens] = useState("");
  const [done, setDone] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const unlisten: (() => void)[] = [];
    listen<string>("llm:token", (e) => setTokens((p) => p + e.payload)).then((f) =>
      unlisten.push(f)
    );
    listen("llm:done", () => setDone(true)).then((f) => unlisten.push(f));
    listen<string>("llm:error", (e) => setError(e.payload)).then((f) =>
      unlisten.push(f)
    );
    return () => unlisten.forEach((fn) => fn());
  }, []);

  const send = useCallback(
    async (
      messages: ChatMessage[],
      provider: string,
      model: string,
      options?: ChatOptions
    ) => {
      setTokens("");
      setDone(false);
      setError(null);
      await invoke("chat_stream", {
        provider,
        model,
        messages,
        stream: options?.stream ?? true,
        temperature: options?.temperature,
        maxTokens: options?.maxTokens,
      });
    },
    []
  );

  const stop = useCallback(() => {
    setDone(true);
  }, []);

  return { tokens, done, error, send, stop, setTokens };
}
