import { useState } from "react";

import AgentSelector from "./components/AgentSelector";
import ChatZone from "./components/ChatZone";
import FileManager from "./components/FileManager";
import PluginSlot from "./components/PluginSlot";
import ProviderSelector from "./components/ProviderSelector";
import SettingsDialog from "./components/SettingsDialog";
import SidePanel from "./components/SidePanel";
import Splitter from "./components/layout/Splitter";
import { AppProvider } from "./context/AppContext";
import { SessionsProvider } from "./context/SessionsContext";
import { LAYOUT_DEFAUT, useLayout } from "./hooks/useLayout";
import "./App.css";

/**
 * Chevron de repli, posé en haut de chaque zone.
 *
 * Les deux boutons « Control » et « Files » vivaient dans la barre du haut,
 * loin de ce qu'ils commandaient. Un chevron au coin de sa propre zone dit de
 * lui-même ce qu'il replie.
 */
function ZoneToggle({
  open,
  onToggle,
  side,
  label,
}: {
  open: boolean;
  onToggle: () => void;
  side: "left" | "right";
  label: string;
}): React.JSX.Element {
  const ferme = side === "left" ? "◀" : "▶";
  const ouvre = side === "left" ? "▶" : "◀";
  return (
    <button
      type="button"
      className={`zone-toggle zone-toggle-${side}`}
      onClick={onToggle}
      aria-expanded={open}
      aria-label={open ? `Replier ${label}` : `Déplier ${label}`}
      title={open ? `Replier ${label}` : `Déplier ${label}`}
    >
      {open ? ferme : ouvre}
    </button>
  );
}

export default function App() {
  const { layout, setLayout } = useLayout();
  const [reglages, setReglages] = useState(false);

  return (
    <AppProvider>
      <SessionsProvider>
        <div className="layout">
          <header className="topbar">
            <div className="topbar-left">
              {!layout.leftOpen && (
                <ZoneToggle
                  open={false}
                  side="left"
                  label="le panneau"
                  onToggle={() => setLayout({ leftOpen: true })}
                />
              )}
              <AgentSelector />
            </div>
            <div className="topbar-right">
              <ProviderSelector />
              <button
                type="button"
                className="btn btn-ghost icon-btn"
                onClick={() => setReglages(true)}
                aria-label="Réglages"
                title="Réglages"
              >
                ⚙
              </button>
              {!layout.rightOpen && (
                <ZoneToggle
                  open={false}
                  side="right"
                  label="le coffre"
                  onToggle={() => setLayout({ rightOpen: true })}
                />
              )}
            </div>
          </header>

          <div className="zones">
            {layout.leftOpen && (
              <>
                <aside
                  className="zone zone-left"
                  style={{ width: layout.leftWidth }}
                  aria-label="Panneau de travail"
                >
                  <ZoneToggle
                    open
                    side="left"
                    label="le panneau"
                    onToggle={() => setLayout({ leftOpen: false })}
                  />
                  <SidePanel />
                </aside>
                <Splitter
                  value={layout.leftWidth}
                  onChange={(leftWidth) => setLayout({ leftWidth })}
                  defaultValue={LAYOUT_DEFAUT.leftWidth}
                  sign={1}
                  label="Largeur du panneau de travail"
                />
              </>
            )}

            <main className="zone zone-center">
              <ChatZone />
            </main>

            {layout.rightOpen && (
              <>
                <Splitter
                  value={layout.rightWidth}
                  onChange={(rightWidth) => setLayout({ rightWidth })}
                  defaultValue={LAYOUT_DEFAUT.rightWidth}
                  sign={-1}
                  label="Largeur du coffre"
                />
                <aside
                  className="zone zone-right"
                  style={{ width: layout.rightWidth }}
                  aria-label="Coffre"
                >
                  <ZoneToggle
                    open
                    side="right"
                    label="le coffre"
                    onToggle={() => setLayout({ rightOpen: false })}
                  />
                  <FileManager />
                </aside>
              </>
            )}
          </div>

          <PluginSlot point="status-bar" />
        </div>

        {reglages && <SettingsDialog onClose={() => setReglages(false)} />}
      </SessionsProvider>
    </AppProvider>
  );
}
