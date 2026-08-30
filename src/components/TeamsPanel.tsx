/**
 * Équipes d'agents — composition et lancement.
 *
 * Une équipe assemble des agents **du coffre** en leur donnant chacun son
 * modèle : un rôle bavard peut tourner en local pendant qu'un rôle décisif
 * utilise un modèle plus capable. L'équipe elle-même n'est pas écrite dans le
 * coffre — celui-ci reste utilisable par n'importe quel autre harness.
 */
import { useState } from "react";

import { useApp } from "../context/AppContext";
import { useSessions } from "../context/SessionsContext";
import * as ipc from "../lib/ipc";
import type { TeamMember, TeamStrategy } from "../types/ipc";

interface Brouillon {
  name: string;
  description: string;
  strategy: TeamStrategy;
  maxTurns: number;
  members: TeamMember[];
}

const VIDE: Brouillon = {
  name: "",
  description: "",
  strategy: "round-robin",
  maxTurns: 6,
  members: [],
};

export default function TeamsPanel(): React.JSX.Element {
  const { agents, providers } = useApp();
  const { teams, refreshTeams, createTeamSession } = useSessions();

  const [ouvert, setOuvert] = useState(false);
  const [brouillon, setBrouillon] = useState<Brouillon>(VIDE);
  const [erreur, setErreur] = useState<string | null>(null);
  const [enCours, setEnCours] = useState(false);

  // Toutes les paires fournisseur/modèle joignables, pour le choix par membre.
  const choix = providers.flatMap((p) =>
    p.models.map((m) => ({ provider: p.id, model: m.id, label: `${p.id} · ${m.name || m.id}` })),
  );

  const ajouterMembre = (): void => {
    const libres = agents.filter((a) => !brouillon.members.some((m) => m.agent === a.name));
    const premier = libres[0];
    if (!premier || !choix[0]) return;
    setBrouillon((b) => ({
      ...b,
      members: [
        ...b.members,
        { agent: premier.name, provider: choix[0].provider, model: choix[0].model, role: "" },
      ],
    }));
  };

  const majMembre = (i: number, patch: Partial<TeamMember>): void => {
    setBrouillon((b) => ({
      ...b,
      members: b.members.map((m, j) => (j === i ? { ...m, ...patch } : m)),
    }));
  };

  const retirerMembre = (i: number): void => {
    setBrouillon((b) => ({ ...b, members: b.members.filter((_, j) => j !== i) }));
  };

  const enregistrer = async (): Promise<void> => {
    setEnCours(true);
    setErreur(null);
    try {
      await ipc.teamSave({
        name: brouillon.name,
        description: brouillon.description,
        strategy: brouillon.strategy,
        maxTurns: brouillon.maxTurns,
        members: brouillon.members.map((m) => ({ ...m, role: m.role?.trim() || null })),
      });
      await refreshTeams();
      setBrouillon(VIDE);
      setOuvert(false);
    } catch (e) {
      // Le backend porte les règles (membres uniques, superviseur accompagné,
      // budget borné) : on affiche son message plutôt que de les redire ici.
      setErreur(e instanceof Error ? e.message : String(e));
    } finally {
      setEnCours(false);
    }
  };

  const supprimer = async (id: string): Promise<void> => {
    await ipc.teamDelete(id);
    await refreshTeams();
  };

  return (
    <section className="panel-section">
      <div className="dropdown-head">
        <span>Équipes</span>
        <button type="button" className="link" onClick={() => setOuvert((v) => !v)}>
          {ouvert ? "Annuler" : "Composer"}
        </button>
      </div>

      {teams.length === 0 && !ouvert && (
        <p className="empty-hint">
          Aucune équipe. Composez-en une pour faire travailler plusieurs agents
          sur un même objectif.
        </p>
      )}

      {teams.map((t) => (
        <div className="team-row" key={t.id}>
          <div className="team-body">
            <div className="team-title">{t.name}</div>
            <div className="runtime-meta">
              {t.strategy === "supervisor" ? "superviseur" : "tour de rôle"} ·{" "}
              {t.members.map((m) => m.agent).join(", ")} · {t.maxTurns} tours max
            </div>
          </div>
          <button
            type="button"
            className="btn btn-mini"
            onClick={() => void createTeamSession(t.id)}
            title="Ouvrir une session avec cette équipe"
          >
            Session
          </button>
          <button
            type="button"
            className="session-close"
            aria-label={`Supprimer ${t.name}`}
            onClick={() => void supprimer(t.id)}
          >
            ×
          </button>
        </div>
      ))}

      {ouvert && (
        <div className="team-form">
          <label>
            Nom
            <input
              value={brouillon.name}
              onChange={(e) => setBrouillon((b) => ({ ...b, name: e.target.value }))}
            />
          </label>
          <label>
            Objet de l'équipe
            <input
              value={brouillon.description}
              onChange={(e) => setBrouillon((b) => ({ ...b, description: e.target.value }))}
            />
          </label>
          <label>
            Tour de parole
            <select
              value={brouillon.strategy}
              onChange={(e) =>
                setBrouillon((b) => ({ ...b, strategy: e.target.value as TeamStrategy }))
              }
            >
              <option value="round-robin">Tour de rôle</option>
              <option value="supervisor">Superviseur (le 1er membre distribue)</option>
            </select>
          </label>
          <label>
            Tours maximum
            <input
              type="number"
              min={1}
              max={50}
              value={brouillon.maxTurns}
              onChange={(e) =>
                setBrouillon((b) => ({ ...b, maxTurns: Number(e.target.value) || 1 }))
              }
            />
          </label>

          {brouillon.members.map((m, i) => (
            <div className="team-member" key={i}>
              <select
                value={m.agent}
                onChange={(e) => majMembre(i, { agent: e.target.value })}
                aria-label="Agent"
              >
                {agents.map((a) => (
                  <option key={a.name} value={a.name}>
                    {a.name}
                  </option>
                ))}
              </select>
              <select
                value={`${m.provider ?? ""}|${m.model}`}
                onChange={(e) => {
                  const [provider, model] = e.target.value.split("|");
                  majMembre(i, { provider: provider || null, model });
                }}
                aria-label="Modèle"
              >
                {choix.map((c) => (
                  <option key={`${c.provider}|${c.model}`} value={`${c.provider}|${c.model}`}>
                    {c.label}
                  </option>
                ))}
              </select>
              <input
                placeholder="rôle dans l'équipe"
                value={m.role ?? ""}
                onChange={(e) => majMembre(i, { role: e.target.value })}
              />
              <button
                type="button"
                className="session-close"
                aria-label="Retirer ce membre"
                onClick={() => retirerMembre(i)}
              >
                ×
              </button>
            </div>
          ))}

          <div className="team-actions">
            <button
              type="button"
              className="btn btn-mini"
              onClick={ajouterMembre}
              disabled={brouillon.members.length >= agents.length || choix.length === 0}
            >
              + Membre
            </button>
            <button
              type="button"
              className="btn btn-primary btn-mini"
              onClick={() => void enregistrer()}
              disabled={enCours || !brouillon.name.trim() || brouillon.members.length === 0}
            >
              {enCours ? "…" : "Enregistrer"}
            </button>
          </div>

          {choix.length === 0 && (
            <p className="empty-hint">
              Aucun modèle disponible : lancez d'abord la détection des moteurs.
            </p>
          )}
          {erreur && <p className="err-text">{erreur}</p>}
        </div>
      )}
    </section>
  );
}
