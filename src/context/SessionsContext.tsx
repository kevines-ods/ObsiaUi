/**
 * Sessions de conversation — état partagé.
 *
 * Plusieurs sessions peuvent streamer **en même temps**. Tout l'état volatil
 * est donc indexé par identifiant de session (`Record<string, …>`) plutôt que
 * stocké pour « la » conversation courante : changer d'onglet pendant qu'une
 * réponse arrive ne doit ni l'interrompre ni la perdre.
 *
 * Un seul abonnement couvre toutes les sessions : les événements `session:*`
 * portent leur `sessionId`, on route dessus.
 */
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

import { useApp } from "./AppContext";
import * as ipc from "../lib/ipc";
import type {
  Session,
  SessionMessage,
  SessionMeta,
  Team,
  VaultEntry,
} from "../types/ipc";

interface SessionsContextValue {
  sessions: SessionMeta[];
  activeId: string | null;
  active: Session | null;
  loading: boolean;
  /** Texte en cours de réception, par session. */
  streaming: Record<string, string>;
  /** Sessions dont un tour est en cours. */
  busy: Record<string, boolean>;
  /** Dernière erreur, par session. */
  errors: Record<string, string | null>;
  /** Membre ayant la parole, par session d'équipe. */
  speaking: Record<string, string | null>;
  teams: Team[];
  refreshTeams: () => Promise<void>;
  createSession: () => Promise<void>;
  createTeamSession: (teamId: string) => Promise<void>;
  selectSession: (id: string) => void;
  renameSession: (id: string, title: string) => Promise<void>;
  deleteSession: (id: string) => Promise<void>;
  send: (content: string) => Promise<void>;
  cancel: (id: string) => Promise<void>;
  exportSession: (id: string, project: string) => Promise<VaultEntry>;
}

const SessionsContext = createContext<SessionsContextValue | null>(null);

export function useSessions(): SessionsContextValue {
  const ctx = useContext(SessionsContext);
  if (!ctx) {
    throw new Error("useSessions doit être utilisé dans <SessionsProvider>");
  }
  return ctx;
}

const message = (e: unknown): string => (e instanceof Error ? e.message : String(e));

export function SessionsProvider({ children }: { children: ReactNode }): React.JSX.Element {
  const { selectedProviderId, selectedModel, selectedAgent } = useApp();

  const [sessions, setSessions] = useState<SessionMeta[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [active, setActive] = useState<Session | null>(null);
  const [loading, setLoading] = useState(true);
  const [streaming, setStreaming] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<Record<string, boolean>>({});
  const [errors, setErrors] = useState<Record<string, string | null>>({});
  const [speaking, setSpeaking] = useState<Record<string, string | null>>({});
  const [teams, setTeams] = useState<Team[]>([]);

  // L'abonnement aux événements est monté une seule fois : il doit lire la
  // session active au moment où l'événement arrive, pas celle capturée à
  // l'abonnement — d'où la ref.
  const activeIdRef = useRef<string | null>(null);
  const hydrated = useRef(false);

  // Écrire la ref dans un effet, pas pendant le rendu : React peut rejouer un
  // rendu sans le valider, ce qui laisserait la ref désynchronisée.
  useEffect(() => {
    activeIdRef.current = activeId;
  }, [activeId]);

  const refresh = useCallback(async (): Promise<SessionMeta[]> => {
    const list = await ipc.sessionsList();
    setSessions(list);
    return list;
  }, []);

  const refreshTeams = useCallback(async (): Promise<void> => {
    try {
      setTeams(await ipc.teamsList());
    } catch (e) {
      setErrors((prev) => ({ ...prev, __global: message(e) }));
    }
  }, []);

  const loadSession = useCallback(async (id: string): Promise<void> => {
    try {
      setActive(await ipc.sessionGet(id));
    } catch (e) {
      setErrors((prev) => ({ ...prev, [id]: message(e) }));
      setActive(null);
    }
  }, []);

  const selectSession = useCallback((id: string): void => {
    setActiveId(id);
  }, []);

  // Hydratation : liste des sessions, puis ouverture de la plus récente.
  useEffect(() => {
    if (hydrated.current) return;
    hydrated.current = true;
    void (async () => {
      try {
        const list = await refresh();
        await refreshTeams();
        if (list.length > 0) setActiveId(list[0].id);
      } catch (e) {
        setErrors((prev) => ({ ...prev, __global: message(e) }));
      } finally {
        setLoading(false);
      }
    })();
  }, [refresh, refreshTeams]);

  // Chargement de l'historique quand l'onglet actif change.
  useEffect(() => {
    if (!activeId) {
      setActive(null);
      return;
    }
    void loadSession(activeId);
  }, [activeId, loadSession]);

  // Abonnement unique, routé par sessionId.
  useEffect(() => {
    let unlisten: Array<() => void> = [];
    let cancelled = false;

    void (async () => {
      const subs = await Promise.all([
        ipc.sessionStream.onToken(({ sessionId, token }) => {
          setStreaming((prev) => ({
            ...prev,
            [sessionId]: (prev[sessionId] ?? "") + token,
          }));
        }),
        ipc.sessionStream.onTurn(({ sessionId, agent }) => {
          // Nouveau membre : on repart d'une bulle vide à son nom.
          setSpeaking((prev) => ({ ...prev, [sessionId]: agent }));
          setStreaming((prev) => ({ ...prev, [sessionId]: "" }));
        }),
        ipc.sessionStream.onMessage(({ sessionId, message: msg, meta }) => {
          // Intervention close : elle rejoint le fil, la bulle en cours se vide.
          setStreaming((prev) => ({ ...prev, [sessionId]: "" }));
          setSessions((prev) =>
            prev
              .map((s) => (s.id === meta.id ? meta : s))
              .sort((a, b) => b.updatedAt - a.updatedAt),
          );
          if (activeIdRef.current === sessionId) {
            setActive((prev) =>
              prev && prev.id === sessionId
                ? { ...prev, ...meta, messages: [...prev.messages, msg] }
                : prev,
            );
          }
        }),
        ipc.sessionStream.onDone(({ sessionId, message: msg, meta }) => {
          setStreaming((prev) => {
            const copy = { ...prev };
            delete copy[sessionId];
            return copy;
          });
          setBusy((prev) => ({ ...prev, [sessionId]: false }));
          setSpeaking((prev) => ({ ...prev, [sessionId]: null }));
          setSessions((prev) =>
            prev
              .map((s) => (s.id === meta.id ? meta : s))
              .sort((a, b) => b.updatedAt - a.updatedAt),
          );
          setActive((prev) => {
            if (!prev || prev.id !== sessionId) return prev;
            // Un tour annulé avant le premier jeton n'a produit aucun message ;
            // une exécution d'équipe a déjà reçu le sien par `session:message`.
            const deja =
              msg.content.length === 0 ||
              prev.messages.some((m) => m.at === msg.at && m.content === msg.content);
            return deja
              ? { ...prev, ...meta }
              : { ...prev, ...meta, messages: [...prev.messages, msg] };
          });
        }),
        ipc.sessionStream.onError(({ sessionId, error }) => {
          setStreaming((prev) => {
            const copy = { ...prev };
            delete copy[sessionId];
            return copy;
          });
          setBusy((prev) => ({ ...prev, [sessionId]: false }));
          setSpeaking((prev) => ({ ...prev, [sessionId]: null }));
          setErrors((prev) => ({ ...prev, [sessionId]: error }));
        }),
      ]);
      if (cancelled) {
        subs.forEach((u) => u());
        return;
      }
      unlisten = subs;
    })();

    return () => {
      cancelled = true;
      unlisten.forEach((u) => u());
    };
  }, []);

  const createSession = useCallback(async (): Promise<void> => {
    if (!selectedModel) {
      setErrors((prev) => ({ ...prev, __global: "Sélectionnez d'abord un modèle." }));
      return;
    }
    try {
      const meta = await ipc.sessionCreate({
        agent: selectedAgent,
        provider: selectedProviderId || null,
        model: selectedModel,
      });
      setSessions((prev) => [meta, ...prev]);
      setActiveId(meta.id);
      setErrors((prev) => ({ ...prev, __global: null }));
    } catch (e) {
      setErrors((prev) => ({ ...prev, __global: message(e) }));
    }
  }, [selectedAgent, selectedProviderId, selectedModel]);

  const createTeamSession = useCallback(
    async (teamId: string): Promise<void> => {
      const team = teams.find((t) => t.id === teamId);
      if (!team) return;
      try {
        // Le modèle de la session est celui du premier membre : chaque membre
        // parlera ensuite avec le sien.
        const meta = await ipc.sessionCreate({
          team: teamId,
          provider: team.members[0]?.provider ?? null,
          model: team.members[0]?.model ?? "",
        });
        setSessions((prev) => [meta, ...prev]);
        setActiveId(meta.id);
        setErrors((prev) => ({ ...prev, __global: null }));
      } catch (e) {
        setErrors((prev) => ({ ...prev, __global: message(e) }));
      }
    },
    [teams],
  );

  const renameSession = useCallback(async (id: string, title: string): Promise<void> => {
    try {
      const meta = await ipc.sessionRename(id, title);
      setSessions((prev) => prev.map((s) => (s.id === id ? meta : s)));
      setActive((prev) => (prev && prev.id === id ? { ...prev, ...meta } : prev));
    } catch (e) {
      setErrors((prev) => ({ ...prev, [id]: message(e) }));
    }
  }, []);

  const deleteSession = useCallback(
    async (id: string): Promise<void> => {
      try {
        await ipc.sessionDelete(id);
      } catch (e) {
        setErrors((prev) => ({ ...prev, [id]: message(e) }));
        return;
      }
      setSessions((prev) => {
        const reste = prev.filter((s) => s.id !== id);
        // Fermer l'onglet actif bascule sur le voisin le plus récent.
        setActiveId((courant) => (courant === id ? (reste[0]?.id ?? null) : courant));
        return reste;
      });
    },
    [],
  );

  const send = useCallback(
    async (content: string): Promise<void> => {
      const id = activeId;
      const texte = content.trim();
      if (!id || !texte || busy[id]) return;

      // Le message utilisateur est affiché immédiatement : le backend le
      // persiste de son côté, l'écho local évite un aller-retour visible.
      const local: SessionMessage = {
        role: "user",
        content: texte,
        at: Math.floor(Date.now() / 1000),
      };
      setActive((prev) =>
        prev && prev.id === id ? { ...prev, messages: [...prev.messages, local] } : prev,
      );
      setBusy((prev) => ({ ...prev, [id]: true }));
      setErrors((prev) => ({ ...prev, [id]: null }));
      setStreaming((prev) => ({ ...prev, [id]: "" }));

      try {
        // Une session d'équipe fait travailler plusieurs agents à la suite ;
        // le message devient leur objectif commun.
        if (active?.id === id && active.team) {
          await ipc.teamRun(id, texte);
        } else {
          await ipc.sessionSend(id, texte);
        }
      } catch (e) {
        // L'événement session:error a déjà pu passer ; on ne l'écrase que si
        // aucune erreur n'a encore été enregistrée.
        setBusy((prev) => ({ ...prev, [id]: false }));
        setErrors((prev) => ({ ...prev, [id]: prev[id] ?? message(e) }));
      }
    },
    [activeId, busy, active],
  );

  const cancel = useCallback(async (id: string): Promise<void> => {
    try {
      await ipc.sessionCancel(id);
    } catch {
      // L'annulation est au mieux : si le tour est déjà fini, rien à faire.
    }
  }, []);

  const exportSession = useCallback(
    (id: string, project: string): Promise<VaultEntry> => ipc.sessionExport(id, project),
    [],
  );

  const value = useMemo<SessionsContextValue>(
    () => ({
      sessions,
      activeId,
      active,
      loading,
      streaming,
      busy,
      errors,
      speaking,
      teams,
      refreshTeams,
      createSession,
      createTeamSession,
      selectSession,
      renameSession,
      deleteSession,
      send,
      cancel,
      exportSession,
    }),
    [
      sessions,
      activeId,
      active,
      loading,
      streaming,
      busy,
      errors,
      speaking,
      teams,
      refreshTeams,
      createSession,
      createTeamSession,
      selectSession,
      renameSession,
      deleteSession,
      send,
      cancel,
      exportSession,
    ],
  );

  return <SessionsContext.Provider value={value}>{children}</SessionsContext.Provider>;
}
