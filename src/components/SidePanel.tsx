/**
 * Panneau latéral gauche : ce sur quoi on travaille.
 *
 * Quatre sections repliables — sessions, équipes, planification, accès
 * distant. Ce qui relève du réglage plutôt que du travail (fournisseurs, clés,
 * extensions, MCP) est parti dans la fenêtre de réglages : sans ce partage, la
 * colonne devenait un empilement où l'on ne trouvait plus rien.
 */
import PluginSlot from "./PluginSlot";
import PlansPanel from "./PlansPanel";
import RemotePanel from "./RemotePanel";
import SessionsPanel from "./SessionsPanel";
import TeamsPanel from "./TeamsPanel";
import Section from "./layout/Section";
import { useSessions } from "../context/SessionsContext";

export default function SidePanel(): React.JSX.Element {
  const { sessions, teams, busy, createSession } = useSessions();
  const actives = sessions.filter((s) => busy[s.id]).length;

  return (
    <div className="side-panel">
      <Section
        id="sessions"
        title="Sessions"
        defaultOpen
        badge={actives > 0 ? `${actives} en cours` : sessions.length || undefined}
        action={
          <button
            type="button"
            className="section-action"
            onClick={() => void createSession()}
            aria-label="Nouvelle session"
            title="Nouvelle session"
          >
            +
          </button>
        }
      >
        <SessionsPanel />
      </Section>

      <Section id="equipes" title="Équipes" badge={teams.length || undefined}>
        <TeamsPanel />
      </Section>

      <Section id="planification" title="Planification">
        <PlansPanel />
      </Section>

      <Section id="distant" title="À distance">
        <RemotePanel />
      </Section>

      <PluginSlot point="control-panel" />
    </div>
  );
}
