import { useState } from "react";
import AgentSelector from "./components/AgentSelector";
import ChatZone from "./components/ChatZone";
import ControlPanel from "./components/ControlPanel";
import FileManager from "./components/FileManager";
import PluginSlot from "./components/PluginSlot";
import ProviderSelector from "./components/ProviderSelector";
import { AppProvider } from "./context/AppContext";
import { SessionsProvider } from "./context/SessionsContext";
import "./App.css";

export default function App() {
  const [leftOpen, setLeftOpen] = useState(true);
  const [rightOpen, setRightOpen] = useState(true);

  return (
    <AppProvider>
      <SessionsProvider>
      <div className="layout">
      <header className="topbar">
        <div className="topbar-left">
          <button onClick={() => setLeftOpen((v) => !v)} aria-label="Toggle control panel">
            {leftOpen ? "◀" : "▶"} Control
          </button>
          <button onClick={() => setRightOpen((v) => !v)} aria-label="Toggle file manager">
            Files {rightOpen ? "▶" : "◀"}
          </button>
        </div>
        <AgentSelector />
        <ProviderSelector />
      </header>
      <div className="zones">
        {leftOpen && (
          <aside className="zone zone-left">
            <ControlPanel />
          </aside>
        )}
        <main className="zone zone-center">
          <ChatZone />
        </main>
        {rightOpen && (
          <aside className="zone zone-right">
            <FileManager />
          </aside>
        )}
      </div>
      </div>
      <PluginSlot point="status-bar" />
      </SessionsProvider>
    </AppProvider>
  );
}
