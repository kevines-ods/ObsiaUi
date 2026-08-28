/**
 * Hook de streaming LLM — aligné sur le contrat IPC `chat_stream`.
 *
 * - `send(messages, options)` déclenche la commande `chat_stream` avec un
 *   payload `{ provider, model, messages, temperature, maxTokens }`.
 * - S'abonne aux événements `llm:token` (string), `llm:done` (ChatResponse),
 *   `llm:error` (string).
 * - `stop()` : arrêt best-effort côté UI (le backend n'expose pas de commande
 *   d'annulation, on ignore donc les événements restants).
 */
import { useCallback, useEffect, useRef, useState } from "react";

import * as ipc from "../lib/ipc";
import type {
  ChatMessage,
  ChatRequestPayload,
  ChatResponse,
} from "../types/ipc";

export interface UseLlmStreamOptions {
  provider?: string | null;
  model?: string | null;
  temperature?: number;
  maxTokens?: number;
}

export interface UseLlmStream {
  tokens: string;
  isStreaming: boolean;
  isDone: boolean;
  error: string | null;
  response: ChatResponse | null;
  send: (
    messages: ChatMessage[],
    options?: UseLlmStreamOptions,
  ) => Promise<void>;
  stop: () => void;
  reset: () => void;
}

export function useLlmStream(): UseLlmStream {
  const [tokens, setTokens] = useState("");
  const [isStreaming, setIsStreaming] = useState(false);
  const [isDone, setIsDone] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [response, setResponse] = useState<ChatResponse | null>(null);

  const tokensRef = useRef("");
  const activeRef = useRef(false);
  const cancelRef = useRef(false);

  const reset = useCallback((): void => {
    tokensRef.current = "";
    cancelRef.current = false;
    activeRef.current = false;
    setTokens("");
    setError(null);
    setResponse(null);
    setIsStreaming(false);
    setIsDone(true);
  }, []);

  const stop = useCallback((): void => {
    cancelRef.current = true;
    activeRef.current = false;
    setIsStreaming(false);
    setIsDone(true);
  }, []);

  const send = useCallback(
    async (
      messages: ChatMessage[],
      options: UseLlmStreamOptions = {},
    ): Promise<void> => {
      const model = options.model;
      if (!model) {
        setError("Aucun modèle sélectionné.");
        return;
      }

      // Nouvelle session de streaming.
      cancelRef.current = false;
      activeRef.current = true;
      tokensRef.current = "";
      setTokens("");
      setError(null);
      setResponse(null);
      setIsStreaming(true);
      setIsDone(false);

      const payload: ChatRequestPayload = {
        provider: options.provider ?? null,
        model,
        messages,
        temperature: options.temperature,
        maxTokens: options.maxTokens,
      };

      try {
        // chat_stream se termine quand le flux a été entièrement émis.
        await ipc.chatStream(payload);
      } catch (e) {
        if (activeRef.current) {
          setError(e instanceof Error ? e.message : String(e));
          setIsStreaming(false);
          setIsDone(true);
          activeRef.current = false;
        }
      }
    },
    [],
  );

  // Abonnement unique aux événements (survit à la StrictMode double-mount).
  useEffect(() => {
    let tokenUnlisten: (() => void) | undefined;
    let doneUnlisten: (() => void) | undefined;
    let errorUnlisten: (() => void) | undefined;
    let cancelled = false;

    void (async () => {
      const [tok, done, err] = await Promise.all([
        ipc.stream.onToken((tokPayload) => {
          if (!activeRef.current || cancelRef.current) return;
          tokensRef.current += tokPayload;
          setTokens(tokensRef.current);
        }),
        ipc.stream.onDone((resp) => {
          if (cancelRef.current) return;
          // Repli : si aucun token n'a été reçu, on extrait le contenu complet.
          if (!tokensRef.current && resp?.choices?.length) {
            const full = resp.choices[0]?.message?.content ?? "";
            tokensRef.current = full;
            setTokens(full);
          }
          setResponse(resp);
          setIsDone(true);
          setIsStreaming(false);
          activeRef.current = false;
        }),
        ipc.stream.onError((msg) => {
          if (cancelRef.current) return;
          setError(msg);
          setIsDone(true);
          setIsStreaming(false);
          activeRef.current = false;
        }),
      ]);

      if (cancelled) {
        tok();
        done();
        err();
        return;
      }
      tokenUnlisten = tok;
      doneUnlisten = done;
      errorUnlisten = err;
    })();

    return () => {
      cancelled = true;
      tokenUnlisten?.();
      doneUnlisten?.();
      errorUnlisten?.();
    };
  }, []);

  return { tokens, isStreaming, isDone, error, response, send, stop, reset };
}
