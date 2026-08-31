/**
 * Planification : objectifs découpés en étapes assignées.
 *
 * Le panneau montre l'état, pas un fil de messages : une planification n'est
 * pas une conversation mais une structure connue d'avance, dont les étapes
 * indépendantes s'exécutent en parallèle. D'où les pastilles d'étapes, qui
 * disent d'un coup d'œil ce qui tourne, ce qui a abouti et ce qui a été
 * écarté.
 *
 * La création et la modification passent par la fenêtre de configuration
 * (`PlanEditor`), qui offre les deux chemins : écrire les étapes à la main,
 * ou partir d'un découpage proposé par un modèle et le retoucher.
 */
import { useCallback, useEffect, useState } from "react";

import PlanEditor from "./PlanEditor";
import * as ipc from "../lib/ipc";
import type { Plan, PlanStep, StepStatus } from "../types/ipc";

const ETIQUETTE: Record<StepStatus, string> = {
  pending: "en attente",
  running: "en cours",
  done: "faite",
  failed: "échec",
  skipped: "écartée",
};

const CLOSES: StepStatus[] = ["done", "failed", "skipped"];

function EtapeChip({ step }: { step: PlanStep }): React.JSX.Element {
  return (
    <span
      className={`step-chip step-${step.status}`}
      title={`${step.title} — ${ETIQUETTE[step.status]}${step.error ? ` : ${step.error}` : ""}
agent ${step.agent} · ${step.model}${
        step.dependsOn.length ? `\ndépend de ${step.dependsOn.join(", ")}` : ""
      }`}
    >
      {step.id}
    </span>
  );
}

export default function PlansPanel(): React.JSX.Element {
  const [plans, setPlans] = useState<Plan[]>([]);
  const [enCours, setEnCours] = useState<string | null>(null);
  const [erreur, setErreur] = useState<string | null>(null);
  const [edition, setEdition] = useState<{ plan: Plan | null } | null>(null);

  const recharger = useCallback(async (): Promise<void> => {
    try {
      setPlans(await ipc.plansList());
      setErreur(null);
    } catch (e) {
      setErreur(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void recharger();
  }, [recharger]);

  // L'avancement arrive par événement : une exécution parallèle produit
  // plusieurs transitions qu'un rechargement périodique manquerait.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let annule = false;
    void ipc.planStream
      .onUpdate(({ plan }) => {
        setPlans((prev) =>
          prev.some((p) => p.id === plan.id)
            ? prev.map((p) => (p.id === plan.id ? plan : p))
            : [plan, ...prev],
        );
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

  const executer = async (id: string): Promise<void> => {
    setEnCours(id);
    setErreur(null);
    try {
      await ipc.planRun(id);
    } catch (e) {
      setErreur(e instanceof Error ? e.message : String(e));
    } finally {
      setEnCours(null);
      await recharger();
    }
  };

  return (
    <div className="panel-block">
      <div className="panel-actions">
        <button type="button" className="link" onClick={() => setEdition({ plan: null })}>
          Nouvelle
        </button>
      </div>

      {plans.length === 0 && (
        <p className="empty-hint">
          Aucune planification. Découpez un objectif en étapes : celles qui ne
          dépendent de rien s'exécuteront en parallèle.
        </p>
      )}

      {plans.map((p) => {
        const faites = p.steps.filter((s) => CLOSES.includes(s.status)).length;
        const tourne = enCours === p.id || p.status === "running";
        return (
          <div className="plan-row" key={p.id}>
            <div className="plan-head">
              <button
                type="button"
                className="team-title link-plain"
                onClick={() => setEdition({ plan: p })}
                title="Ouvrir la configuration"
              >
                {p.title}
              </button>
              <span className="runtime-meta">
                {faites}/{p.steps.length}
              </span>
              {tourne ? (
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
                      ? "Reprend là où la planification s'est arrêtée"
                      : "Exécuter"
                  }
                >
                  {p.status === "incomplete" ? "Reprendre" : "Exécuter"}
                </button>
              )}
              <button
                type="button"
                className="session-close"
                aria-label={`Supprimer ${p.title}`}
                onClick={() => void ipc.planDelete(p.id).then(recharger)}
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

      {erreur && <p className="err-text">{erreur}</p>}

      {edition && (
        <PlanEditor
          plan={edition.plan}
          onClose={() => setEdition(null)}
          onSaved={() => void recharger()}
        />
      )}
    </div>
  );
}
