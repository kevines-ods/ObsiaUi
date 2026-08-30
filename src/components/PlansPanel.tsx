/**
 * Plans — décomposition d'un objectif en étapes assignées.
 *
 * Un plan n'est pas une conversation : c'est une structure connue d'avance,
 * dont les étapes indépendantes s'exécutent en parallèle. Le panneau montre
 * donc l'état de chaque étape plutôt qu'un fil de messages.
 *
 * Un découpage proposé par un modèle est affiché **avant** d'être enregistré :
 * il mérite d'être relu avant d'engager le budget de son exécution.
 */
import { useEffect, useState } from "react";

import { useApp } from "../context/AppContext";
import * as ipc from "../lib/ipc";
import type { Plan, PlanStep, StepStatus } from "../types/ipc";

const ETIQUETTE: Record<StepStatus, string> = {
  pending: "en attente",
  running: "en cours",
  done: "faite",
  failed: "échec",
  skipped: "écartée",
};

function EtapeChip({ step }: { step: PlanStep }): React.JSX.Element {
  return (
    <span
      className={`step-chip step-${step.status}`}
      title={`${step.title} — ${ETIQUETTE[step.status]}${
        step.error ? ` : ${step.error}` : ""
      }\nagent ${step.agent} · ${step.model}${
        step.dependsOn.length ? `\ndépend de ${step.dependsOn.join(", ")}` : ""
      }`}
    >
      {step.id}
    </span>
  );
}

export default function PlansPanel(): React.JSX.Element {
  const { selectedProviderId, selectedModel, selectedAgent } = useApp();

  const [plans, setPlans] = useState<Plan[]>([]);
  const [draft, setDraft] = useState<Plan | null>(null);
  const [objectif, setObjectif] = useState("");
  const [occupe, setOccupe] = useState(false);
  const [encours, setEncours] = useState<string | null>(null);
  const [erreur, setErreur] = useState<string | null>(null);
  const [ouvert, setOuvert] = useState(false);

  const recharger = async (): Promise<void> => {
    try {
      setPlans(await ipc.plansList());
    } catch (e) {
      setErreur(e instanceof Error ? e.message : String(e));
    }
  };

  useEffect(() => {
    void recharger();
  }, []);

  // L'avancement arrive par événement : une exécution parallèle produit
  // plusieurs transitions qu'un rechargement périodique manquerait.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let annule = false;
    void ipc.planStream
      .onUpdate(({ plan }) => {
        setPlans((prev) => {
          const connu = prev.some((p) => p.id === plan.id);
          return connu ? prev.map((p) => (p.id === plan.id ? plan : p)) : [plan, ...prev];
        });
      })
      .then((u) => {
        if (annule) u();
        else unlisten = u;
      });
    return () => {
      annule = true;
      unlisten?.();
    };
  }, []);

  const decomposer = async (): Promise<void> => {
    if (!objectif.trim() || !selectedModel) return;
    setOccupe(true);
    setErreur(null);
    setDraft(null);
    try {
      setDraft(
        await ipc.planDraft({
          objective: objectif,
          agent: selectedAgent ?? "assistant",
          provider: selectedProviderId || null,
          model: selectedModel,
        }),
      );
    } catch (e) {
      setErreur(e instanceof Error ? e.message : String(e));
    } finally {
      setOccupe(false);
    }
  };

  const enregistrer = async (): Promise<void> => {
    if (!draft) return;
    try {
      await ipc.planSave({
        title: draft.title,
        objective: draft.objective,
        steps: draft.steps,
      });
      setDraft(null);
      setObjectif("");
      await recharger();
    } catch (e) {
      setErreur(e instanceof Error ? e.message : String(e));
    }
  };

  const executer = async (id: string): Promise<void> => {
    setEncours(id);
    setErreur(null);
    try {
      await ipc.planRun(id);
    } catch (e) {
      setErreur(e instanceof Error ? e.message : String(e));
    } finally {
      setEncours(null);
      await recharger();
    }
  };

  const supprimer = async (id: string): Promise<void> => {
    await ipc.planDelete(id);
    await recharger();
  };

  return (
    <section className="panel-section">
      <div className="dropdown-head">
        <span>Plans</span>
        <button type="button" className="link" onClick={() => setOuvert((v) => !v)}>
          {ouvert ? "Fermer" : "Décomposer"}
        </button>
      </div>

      {plans.length === 0 && !ouvert && (
        <p className="empty-hint">
          Aucun plan. Décomposez un objectif en étapes : celles qui ne dépendent
          de rien s'exécuteront en parallèle.
        </p>
      )}

      {plans.map((p) => {
        const faites = p.steps.filter((s) =>
          ["done", "failed", "skipped"].includes(s.status),
        ).length;
        return (
          <div className="plan-row" key={p.id}>
            <div className="plan-head">
              <span className="team-title">{p.title}</span>
              <span className="runtime-meta">
                {faites}/{p.steps.length}
              </span>
              {encours === p.id || p.status === "running" ? (
                <button
                  type="button"
                  className="btn btn-mini"
                  onClick={() => void ipc.planCancel(p.id)}
                >
                  Arrêter
                </button>
              ) : (
                <button
                  type="button"
                  className="btn btn-mini"
                  onClick={() => void executer(p.id)}
                  title={
                    p.status === "incomplete"
                      ? "Reprend là où le plan s'est arrêté"
                      : "Exécuter le plan"
                  }
                >
                  {p.status === "incomplete" ? "Reprendre" : "Exécuter"}
                </button>
              )}
              <button
                type="button"
                className="session-close"
                aria-label={`Supprimer ${p.title}`}
                onClick={() => void supprimer(p.id)}
              >
                ×
              </button>
            </div>
            <div className="step-chips">
              {p.steps.map((s) => (
                <EtapeChip key={s.id} step={s} />
              ))}
            </div>
          </div>
        );
      })}

      {ouvert && (
        <div className="team-form">
          <label>
            Objectif
            <input
              value={objectif}
              placeholder="ex. réécrire le module de détection"
              onChange={(e) => setObjectif(e.target.value)}
            />
          </label>
          <div className="team-actions">
            <button
              type="button"
              className="btn btn-mini"
              onClick={() => void decomposer()}
              disabled={occupe || !objectif.trim() || !selectedModel}
            >
              {occupe ? "…" : "Proposer un découpage"}
            </button>
            {draft && (
              <button
                type="button"
                className="btn btn-primary btn-mini"
                onClick={() => void enregistrer()}
              >
                Enregistrer
              </button>
            )}
          </div>

          {draft && (
            <div className="plan-draft">
              <div className="team-title">{draft.title}</div>
              {draft.steps.map((s) => (
                <div className="runtime-meta" key={s.id}>
                  <strong>{s.id}</strong> — {s.title} · {s.agent}
                  {s.dependsOn.length > 0 && ` · après ${s.dependsOn.join(", ")}`}
                </div>
              ))}
            </div>
          )}
          {!selectedModel && (
            <p className="empty-hint">Sélectionnez d'abord un modèle.</p>
          )}
          {erreur && <p className="err-text">{erreur}</p>}
        </div>
      )}
      {!ouvert && erreur && <p className="err-text">{erreur}</p>}
    </section>
  );
}
