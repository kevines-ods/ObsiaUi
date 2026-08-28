import { useEffect, useState, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

export function useLlmStream() {
  const [tokens, setTokens] = useState("");
  const [done, setDone] = useState(false);
  const [error, setError] = useState<string|null>(null);

  useEffect(() => {
    const unlisten: (()=>void)[] = [];
    listen<string>("llm:token", e=> setTokens(p=>p+e.payload)).then(f=>unlisten.push(f));
    listen("llm:done", ()=> setDone(true)).then(f=>unlisten.push(f));
    listen<string>("llm:error", e=> setError(e.payload)).then(f=>unlisten.push(f));
    return ()=> unlisten.forEach(fn=>fn());
  }, []);

  const send = useCallback(async (prompt: string, provider: string, model: string) => {
    setTokens(""); setDone(false); setError(null);
    await invoke("chat_stream", { prompt, provider, model });
  }, []);

  return { tokens, done, error, send, setTokens };
}
