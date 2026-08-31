/**
 * Section repliable du panneau latéral.
 *
 * Le panneau réunit sessions, équipes, planification et accès distant : tout
 * afficher d'un coup produisait une colonne interminable. Chaque section se
 * réduit donc à son titre, et l'état d'ouverture est retenu d'un lancement à
 * l'autre — on retrouve son écran tel qu'on l'a laissé.
 */
import { useCallback, useEffect, useState, type ReactNode } from "react";

const CLE = "obsia.sections";

function lire(): Record<string, boolean> {
  try {
    const brut = localStorage.getItem(CLE);
    return brut ? (JSON.parse(brut) as Record<string, boolean>) : {};
  } catch {
    return {};
  }
}

function ecrire(etat: Record<string, boolean>): void {
  try {
    localStorage.setItem(CLE, JSON.stringify(etat));
  } catch {
    // Stockage indisponible : l'état vaut pour la session en cours.
  }
}

interface Props {
  id: string;
  title: string;
  /** Ouverture au tout premier lancement, avant tout choix de l'utilisateur. */
  defaultOpen?: boolean;
  /** Compteur ou état affiché à droite du titre, replié compris. */
  badge?: ReactNode;
  /** Action de la section, rendue seulement quand elle est ouverte. */
  action?: ReactNode;
  children: ReactNode;
}

export default function Section({
  id,
  title,
  defaultOpen = false,
  badge,
  action,
  children,
}: Props): React.JSX.Element {
  const [ouvert, setOuvert] = useState<boolean>(() => lire()[id] ?? defaultOpen);

  useEffect(() => {
    const etat = lire();
    etat[id] = ouvert;
    ecrire(etat);
  }, [id, ouvert]);

  const basculer = useCallback(() => setOuvert((v) => !v), []);

  return (
    <section className={`section ${ouvert ? "open" : ""}`}>
      <h3 className="section-head">
        <button
          type="button"
          className="section-toggle"
          onClick={basculer}
          aria-expanded={ouvert}
          aria-controls={`section-${id}`}
        >
          <span className="section-chevron" aria-hidden="true">
            {ouvert ? "▾" : "▸"}
          </span>
          <span className="section-title">{title}</span>
          {badge !== undefined && <span className="section-badge">{badge}</span>}
        </button>
        {ouvert && action}
      </h3>
      {ouvert && (
        <div className="section-body" id={`section-${id}`}>
          {children}
        </div>
      )}
    </section>
  );
}
