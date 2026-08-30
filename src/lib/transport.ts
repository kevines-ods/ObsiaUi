/**
 * Transport des appels et des événements.
 *
 * L'interface ne sait pas si le harness tourne dans son propre processus ou
 * sur une autre machine. Deux cibles, une seule surface :
 *
 * - **locale** — `invoke` / `listen` de Tauri ;
 * - **distante** — `POST /rpc` et une WebSocket unique vers une instance
 *   ObsiaUi qui héberge le coffre et les modèles.
 *
 * Les abonnements sont tenus **ici** plutôt que dans les composants. Basculer
 * de cible réattache donc les mêmes gestionnaires à la nouvelle source, sans
 * que rien en amont ait à se réabonner : le changement d'hôte ne coupe pas les
 * flux en cours d'affichage.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Cible =
  | { kind: "local" }
  | { kind: "remote"; url: string; token: string };

type Handler = (payload: unknown) => void;

let cible: Cible = { kind: "local" };
const handlers = new Map<string, Set<Handler>>();
const detacheursLocaux = new Map<string, UnlistenFn>();
let socket: WebSocket | null = null;
/** Empêche la reconnexion automatique après une bascule volontaire. */
let generation = 0;

const ecouteursEtat = new Set<(c: Cible) => void>();

export function cibleCourante(): Cible {
  return cible;
}

/** Prévient l'interface d'un changement de cible (bandeau, indicateur). */
export function surChangementDeCible(cb: (c: Cible) => void): () => void {
  ecouteursEtat.add(cb);
  return () => ecouteursEtat.delete(cb);
}

/** Normalise une adresse saisie à la main en base HTTP. */
export function normaliserUrl(saisie: string): string {
  const brut = saisie.trim().replace(/\/+$/, "");
  if (!brut) return "";
  const avecSchema = /^https?:\/\//i.test(brut) ? brut : `http://${brut}`;
  return avecSchema;
}

/** Base WebSocket correspondant à une base HTTP. */
export function baseWebSocket(url: string): string {
  return url.replace(/^http/i, "ws");
}

function diffuser(nom: string, payload: unknown): void {
  const cibles = handlers.get(nom);
  if (!cibles) return;
  for (const h of cibles) h(payload);
}

// ===== Appels =====

async function appelDistant<T>(
  command: string,
  args: Record<string, unknown>,
): Promise<T> {
  if (cible.kind !== "remote") throw new Error("cible non distante");
  const reponse = await fetch(`${cible.url}/rpc`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      authorization: `Bearer ${cible.token}`,
    },
    body: JSON.stringify({ command, params: args }),
  });
  // Le corps porte le message d'erreur du harness ; le code HTTP seul ne
  // distinguerait pas un jeton refusé d'une commande mal formée.
  const corps = (await reponse.json().catch(() => null)) as {
    ok?: boolean;
    result?: T;
    error?: string;
  } | null;
  if (!reponse.ok || !corps?.ok) {
    throw new Error(corps?.error ?? `hôte distant : HTTP ${reponse.status}`);
  }
  return corps.result as T;
}

/**
 * Invoque une commande **toujours en local**, quelle que soit la cible.
 *
 * Réservé à ce qui concerne cette machine : état du daemon, jeton, clés
 * d'API. Ces commandes ne sont d'ailleurs pas exposées par le serveur — un
 * client attaché ne doit pas pouvoir reconfigurer son hôte.
 */
export function callLocal<T>(
  command: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  return invoke<T>(command, args);
}

/** Invoque une commande du harness sur la cible courante. */
export function call<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  return cible.kind === "local"
    ? invoke<T>(command, args)
    : appelDistant<T>(command, args);
}

// ===== Événements =====

async function attacherLocal(nom: string): Promise<void> {
  if (detacheursLocaux.has(nom)) return;
  detacheursLocaux.set(
    nom,
    await listen(nom, (e) => diffuser(nom, e.payload)),
  );
}

function ouvrirSocket(): void {
  if (cible.kind !== "remote" || socket) return;
  const mien = generation;
  // Le jeton passe en paramètre : l'API WebSocket du navigateur ne permet pas
  // d'en-tête personnalisé. Il n'apparaît donc que dans l'URL locale.
  const ws = new WebSocket(
    `${baseWebSocket(cible.url)}/ws?token=${encodeURIComponent(cible.token)}`,
  );
  socket = ws;
  ws.onmessage = (e) => {
    try {
      const ev = JSON.parse(String(e.data)) as { name: string; payload: unknown };
      diffuser(ev.name, ev.payload);
    } catch {
      // Un message illisible ne doit pas rompre le flux entier.
    }
  };
  ws.onclose = () => {
    if (socket === ws) socket = null;
    // Reconnexion tant que la cible n'a pas changé : une coupure réseau ne
    // doit pas exiger de rebasculer à la main.
    if (mien === generation && cible.kind === "remote") {
      setTimeout(() => {
        if (mien === generation) ouvrirSocket();
      }, 2000);
    }
  };
}

/**
 * Abonne un gestionnaire à un événement. Le désabonnement rendu ne détache
 * que ce gestionnaire ; la source reste attachée tant qu'il en reste d'autres.
 */
export async function subscribe<T>(
  nom: string,
  handler: (payload: T) => void,
): Promise<() => void> {
  const enveloppe = handler as Handler;
  let jeu = handlers.get(nom);
  if (!jeu) {
    jeu = new Set();
    handlers.set(nom, jeu);
  }
  jeu.add(enveloppe);

  if (cible.kind === "local") {
    await attacherLocal(nom);
  } else {
    ouvrirSocket();
  }

  return () => {
    const courant = handlers.get(nom);
    if (!courant) return;
    courant.delete(enveloppe);
    if (courant.size === 0) {
      handlers.delete(nom);
      detacheursLocaux.get(nom)?.();
      detacheursLocaux.delete(nom);
    }
  };
}

// ===== Bascule de cible =====

function detacherTout(): void {
  generation += 1;
  for (const u of detacheursLocaux.values()) u();
  detacheursLocaux.clear();
  if (socket) {
    socket.onclose = null;
    socket.close();
    socket = null;
  }
}

/** Vérifie qu'un hôte distant répond, avant de s'y attacher. */
export async function verifierHote(url: string): Promise<boolean> {
  try {
    const r = await fetch(`${normaliserUrl(url)}/health`);
    if (!r.ok) return false;
    const corps = (await r.json()) as { service?: string };
    return corps.service === "obsiaui";
  } catch {
    return false;
  }
}

/**
 * Bascule de cible et réattache les abonnements existants à la nouvelle
 * source.
 */
export async function connecter(nouvelle: Cible): Promise<void> {
  detacherTout();
  cible = nouvelle;
  if (nouvelle.kind === "local") {
    for (const nom of handlers.keys()) {
      await attacherLocal(nom);
    }
  } else if (handlers.size > 0) {
    ouvrirSocket();
  }
  for (const cb of ecouteursEtat) cb(cible);
}
