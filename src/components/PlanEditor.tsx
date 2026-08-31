/**
 * Fenêtre de configuration d'une planification.
 *
 * Jusqu'ici un plan ne pouvait naître que d'un découpage proposé par un
 * modèle : on acceptait ou on refusait, sans rien pouvoir corriger. Cet
 * éditeur permet les deux chemins — écrire les étapes à la main, ou partir
 * d'une proposition puis la retoucher.
 *
 * Les dépendances se cochent plutôt que se saisissent : un identifiant tapé à
 * la main se trompe, et une dépendance vers une étape inexistante ne se voit
 * qu'à l'enregistrement. Les cycles, eux, restent détectés par le backend —
 * ils ne se repèrent pas à l'œil sur un plan d'une dizaine d'étapes.
 */
import { useState } from "react";

import { useApp } from "../context/AppContext";
import * as ipc from "../lib/ipc";
import type { Plan, PlanStep } from "../types/ipc";

interface Props {
  /** Plan à modifier, ou `null` pour en créer un. */
  plan: Plan | null;
  onClose: () => void;
  onSaved: () => void;
}

function etapeVide(index: number, agent: string, provider: string | null, model: string): PlanStep {
  return {
    id: `s${index}`,
    title: "",
    instruction: "",
    agent,
    provider,
    model,
    dependsOn: [],
    status: "pending",
    result: null,
    error: null,
    startedAt: null,
    finishedAt: null,
  };
}

export default function PlanEditor({ plan, onClose, onSaved }: Props): React.JSX.Element {
  const { agents, providers, selectedProviderId, selectedModel, selectedAgent } = useApp();

  const agentDefaut = selectedAgent ?? agents[0]?.name ?? "assistant";
  const [titre, setTitre] = useState(plan?.title ?? "");
  const [objectif, setObjectif] = useState(plan?.objective ?? "");
  const [etapes, setEtapes] = useState<PlanStep[]>(plan?.steps ?? []);
  const [occupe, setOccupe] = useState(false);
  const [erreur, setErreur] = useState<string | null>(null);

  const choix = providers.flatMap((p) =>
    p.models.map((m) => ({
      provider: p.id,
      model: m.id,
      label: `${p.id} · ${m.name || m.id}`,
    })),
  );

  const ajouter = (): void => {
    // Un identifiant libre, même après des suppressions au milieu.
    let n = etapes.length + 1;
    while (etapes.some((e) => e.id === `s${n}`)) n += 1;
    setEtapes((prev) => [
      ...prev,
      etapeVide(n, agentDefaut, selectedProviderId || null, selectedModel || choix[0]?.model || ""),
    ]);
  };

  const maj = (i: number, patch: Partial<PlanStep>): void =>
    setEtapes((prev) => prev.map((e, j) => (j === i ? { ...e, ...patch } : e)));

  const retirer = (i: number): void =>
    setEtapes((prev) => {
      const partant = prev[i].id;
      // Les dépendances vers l'étape retirée disparaissent avec elle, sinon
      // l'enregistrement échouerait sur une référence orpheline.
      return prev
        .filter((_, j) => j !== i)
        .map((e) => ({ ...e, dependsOn: e.dependsOn.filter((d) => d !== partant) }));
    });

  const basculerDependance = (i: number, id: string): void =>
    setEtapes((prev) =>
      prev.map((e, j) =>
        j === i
          ? {
              ...e,
              dependsOn: e.dependsOn.includes(id)
                ? e.dependsOn.filter((d) => d !== id)
                : [...e.dependsOn, id],
            }
          : e,
      ),
    );

  const proposer = async (): Promise<void> => {
    if (!objectif.trim() || !selectedModel) return;
    setOccupe(true);
    setErreur(null);
    try {
      const brouillon = await ipc.planDraft({
        objective: objectif,
        agent: agentDefaut,
        provider: selectedProviderId || null,
        model: selectedModel,
      });
      setEtapes(brouillon.steps);
      if (!titre.trim()) setTitre(brouillon.title);
    } catch (e) {
      setErreur(e instanceof Error ? e.message : String(e));
    } finally {
      setOccupe(false);
    }
  };

  const enregistrer = async (): Promise<void> => {
    setOccupe(true);
    setErreur(null);
    try {
      await ipc.planSave({
        id: plan?.id ?? null,
        title: titre,
        objective: objectif,
        steps: etapes,
      });
      onSaved();
      onClose();
    } catch (e) {
      // Le backend porte les règles — identifiants uniques, dépendances
      // existantes, absence de cycle — et son message les nomme.
      setErreur(e instanceof Error ? e.message : String(e));
    } finally {
      setOccupe(false);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose} role="presentation">
      <div
        className="modal"
        role="dialog"
        aria-modal="true"
        aria-label={plan ? "Modifier la planification" : "Nouvelle planification"}
        onClick={(e) => e.stopPropagation()}
      >
        <header className="modal-head">
          <h2>{plan ? "Modifier la planification" : "Nouvelle planification"}</h2>
          <button type="button" className="session-close" onClick={onClose} aria-label="Fermer">
            ×
          </button>
        </header>

        <div className="modal-body settings-body">
          <label className="field">
            <span className="label">Titre</span>
            <input value={titre} onChange={(e) => setTitre(e.target.value)} />
          </label>

          <label className="field">
            <span className="label">Objectif</span>
            <textarea
              rows={2}
              value={objectif}
              placeholder="ce que la planification doit accomplir"
              onChange={(e) => setObjectif(e.target.value)}
            />
          </label>

          <div className="team-actions">
            <button
              type="button"
              className="btn btn-mini"
              onClick={() => void proposer()}
              disabled={occupe || !objectif.trim() || !selectedModel}
              title="Un modèle propose un découpage — il reste modifiable ensuite"
            >
              {occupe ? "…" : "Proposer un découpage"}
            </button>
            <button type="button" className="btn btn-mini" onClick={ajouter}>
              + Étape
            </button>
            <span className="runtime-meta">
              {etapes.length} étape(s) · les étapes sans dépendance s'exécutent
              en parallèle
            </span>
          </div>

          {etapes.map((etape, i) => (
            <fieldset className="field-group" key={i}>
              <legend>
                {etape.id || `étape ${i + 1}`}
                <button
                  type="button"
                  className="session-close"
                  onClick={() => retirer(i)}
                  aria-label={`Retirer l'étape ${etape.id}`}
                >
                  ×
                </button>
              </legend>

              <div className="team-member">
                <input
                  value={etape.id}
                  placeholder="identifiant"
                  aria-label="Identifiant de l'étape"
                  onChange={(e) => maj(i, { id: e.target.value })}
                />
                <input
                  value={etape.title}
                  placeholder="titre"
                  aria-label="Titre de l'étape"
                  onChange={(e) => maj(i, { title: e.target.value })}
                />
              </div>

              <label className="field">
                <span className="label">Consigne</span>
                <textarea
                  rows={2}
                  value={etape.instruction}
                  onChange={(e) => maj(i, { instruction: e.target.value })}
                />
              </label>

              <div className="team-member">
                <select
                  value={etape.agent}
                  aria-label="Agent"
                  onChange={(e) => maj(i, { agent: e.target.value })}
                >
                  {agents.map((a) => (
                    <option key={a.name} value={a.name}>
                      {a.name}
                    </option>
                  ))}
                </select>
                <select
                  value={`${etape.provider ?? ""}|${etape.model}`}
                  aria-label="Modèle"
                  onChange={(e) => {
                    const [provider, model] = e.target.value.split("|");
                    maj(i, { provider: provider || null, model });
                  }}
                >
                  {choix.map((c) => (
                    <option key={`${c.provider}|${c.model}`} value={`${c.provider}|${c.model}`}>
                      {c.label}
                    </option>
                  ))}
                </select>
              </div>

              {etapes.length > 1 && (
                <div className="field">
                  <span className="label">Dépend de</span>
                  <div className="team-actions">
                    {etapes
                      .filter((_, j) => j !== i)
                      .map((autre) => (
                        <label key={autre.id} className="theme-choice">
                          <input
                            type="checkbox"
                            checked={etape.dependsOn.includes(autre.id)}
                            onChange={() => basculerDependance(i, autre.id)}
                          />
                          <span>{autre.id}</span>
                        </label>
                      ))}
                  </div>
                </div>
              )}
            </fieldset>
          ))}

          {erreur && <p className="err-text">{erreur}</p>}

          <div className="team-actions">
            <button
              type="button"
              className="btn btn-primary btn-mini"
              onClick={() => void enregistrer()}
              disabled={occupe || !titre.trim() || !objectif.trim() || etapes.length === 0}
            >
              {occupe ? "…" : "Enregistrer"}
            </button>
            <button type="button" className="btn btn-mini" onClick={onClose}>
              Annuler
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
