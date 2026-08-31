/**
 * Fenêtre de réglages.
 *
 * Tout ce qui relève du réglage plutôt que du travail : fournisseurs et clés,
 * extensions, outils MCP, préférences générales. Le panneau latéral gauche
 * garde les sessions, les équipes, la planification et l'accès distant.
 *
 * Sans ce partage, la colonne de gauche accumulait tout et devenait un
 * empilement où l'on ne trouvait plus rien.
 */
import { useEffect, useState } from "react";

import PluginsPanel from "./PluginsPanel";
import GeneralSettings from "./settings/GeneralSettings";
import McpSettings from "./settings/McpSettings";
import ProvidersSettings from "./settings/ProvidersSettings";

type Onglet = "general" | "fournisseurs" | "mcp" | "extensions";

const ONGLETS: Array<{ id: Onglet; libelle: string }> = [
  { id: "general", libelle: "Général" },
  { id: "fournisseurs", libelle: "Fournisseurs" },
  { id: "mcp", libelle: "MCP" },
  { id: "extensions", libelle: "Extensions" },
];

export default function SettingsDialog({
  onClose,
}: {
  onClose: () => void;
}): React.JSX.Element {
  const [onglet, setOnglet] = useState<Onglet>("general");

  // Échap ferme : sur une fenêtre modale, c'est le geste attendu.
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      className="modal-backdrop"
      onClick={onClose}
      role="presentation"
    >
      <div
        className="modal"
        role="dialog"
        aria-modal="true"
        aria-label="Réglages"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="modal-head">
          <h2>Réglages</h2>
          <button
            type="button"
            className="session-close"
            onClick={onClose}
            aria-label="Fermer les réglages"
          >
            ×
          </button>
        </header>

        <nav className="modal-tabs" role="tablist">
          {ONGLETS.map((o) => (
            <button
              key={o.id}
              type="button"
              role="tab"
              aria-selected={onglet === o.id}
              className={`modal-tab ${onglet === o.id ? "active" : ""}`}
              onClick={() => setOnglet(o.id)}
            >
              {o.libelle}
            </button>
          ))}
        </nav>

        <div className="modal-body">
          {onglet === "general" && <GeneralSettings />}
          {onglet === "fournisseurs" && <ProvidersSettings />}
          {onglet === "mcp" && <McpSettings />}
          {onglet === "extensions" && <PluginsPanel />}
        </div>
      </div>
    </div>
  );
}
