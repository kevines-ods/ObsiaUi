import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type Provider = { id: string; label: string; models: string[] };
const FALLBACK: Provider[] = [
  { id: "ollama", label: "Ollama (local)", models: ["llama3.2", "gemma2"] },
  { id: "openai", label: "OpenAI", models: ["gpt-4o", "gpt-4o-mini"] },
  { id: "anthropic", label: "Anthropic", models: ["claude-3-5-sonnet-20241022"] },
  { id: "openrouter", label: "OpenRouter", models: ["auto"] },
  { id: "gemini", label: "Gemini", models: ["gemini-1.5-pro"] },
];

export default function ProviderSelector() {
  const [providers, setProviders] = useState<Provider[]>(FALLBACK);
  const [selectedProvider, setSelectedProvider] = useState(FALLBACK[0].id);
  const [selectedModel, setSelectedModel] = useState(FALLBACK[0].models[0]);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    invoke<Provider[]>("list_providers").then(setProviders).catch(()=>{});
  }, []);

  const current = providers.find(p=>p.id===selectedProvider) ?? providers[0];

  return (
    <div className="provider-selector">
      <button className="provider-btn" onClick={()=>setOpen(v=>!v)}>
        {current.label} / {selectedModel} ▾
      </button>
      {open && (
        <div className="provider-dropdown">
          {providers.map(p=>(
            <div key={p.id} className="provider-group">
              <strong onClick={()=>{setSelectedProvider(p.id); setSelectedModel(p.models[0]);}}>{p.label}</strong>
              <div className="model-list">
                {p.models.map(m=>(
                  <button key={m} className={selectedProvider===p.id&&selectedModel===m?"active":""} onClick={()=>{setSelectedProvider(p.id); setSelectedModel(m); setOpen(false);}}>{m}</button>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
