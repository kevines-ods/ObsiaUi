import { useState } from "react";
import ChatZone from "./components/ChatZone";
import ControlPanel from "./components/ControlPanel";
import FileManager from "./components/FileManager";
import ProviderSelector from "./components/ProviderSelector";
import { AppProvider } from "./context/AppContext";
import "./App.css";

export default function App() {
  const [leftOpen, setLeftOpen] = useState(true);
  const [rightOpen, setRightOpen] = useState(true);

  return (
    <AppProvider>
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
    </AppProvider>
  );
}
