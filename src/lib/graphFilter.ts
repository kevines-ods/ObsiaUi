/**
 * Sélection de ce qu'on affiche du graphe du coffre.
 *
 * Séparé du composant parce que ce sont des fonctions pures sur des
 * ensembles : elles se relisent seules, et le composant n'a plus qu'à les
 * enchaîner dans le bon ordre.
 *
 * L'ordre compte. Les filtres s'appliquent **avant** le voisinage : sinon un
 * dossier masqué servirait quand même de pont entre deux notes, et le
 * voisinage afficherait des liens passant par des nœuds invisibles.
 */
import type { GraphEdge, GraphNode } from "../types/ipc";

/** Retire les accents et la casse : « mémoire » se trouve en tapant « memoire ». */
export function normaliser(texte: string): string {
  return texte
    .normalize("NFD")
    .replace(/\p{Diacritic}/gu, "")
    .toLocaleLowerCase();
}

/**
 * Une note correspond-elle à la recherche ?
 *
 * Le chemin compte autant que le nom : dans un coffre, `sommaire.md` existe
 * dix fois et seul son dossier le distingue.
 */
export function correspond(n: GraphNode, requete: string): boolean {
  const q = normaliser(requete.trim());
  if (!q) return true;
  return (
    normaliser(n.name).includes(q) ||
    normaliser(n.id).includes(q) ||
    n.tags.some((t) => normaliser(t).includes(q))
  );
}

/** Liste d'adjacence non orientée : le voisinage ignore le sens du lien. */
export function adjacence(edges: GraphEdge[]): Map<string, string[]> {
  const adj = new Map<string, string[]>();
  const ajouter = (a: string, b: string): void => {
    const l = adj.get(a);
    if (l) l.push(b);
    else adj.set(a, [b]);
  };
  for (const e of edges) {
    ajouter(e.from, e.to);
    ajouter(e.to, e.from);
  }
  return adj;
}

/**
 * Notes à `profondeur` liens ou moins d'un centre, le centre compris.
 *
 * Un centre absent du graphe rend l'ensemble vide plutôt que le graphe
 * entier : afficher tout le coffre parce qu'une note a été supprimée serait
 * la mauvaise surprise.
 */
export function voisinage(
  centre: string,
  edges: GraphEdge[],
  profondeur: number,
): Set<string> {
  const adj = adjacence(edges);
  const vus = new Set<string>([centre]);
  let front = [centre];
  for (let d = 0; d < profondeur; d += 1) {
    const suivant: string[] = [];
    for (const id of front) {
      for (const voisin of adj.get(id) ?? []) {
        if (!vus.has(voisin)) {
          vus.add(voisin);
          suivant.push(voisin);
        }
      }
    }
    if (suivant.length === 0) break;
    front = suivant;
  }
  return vus;
}

export interface Criteres {
  /** Dossiers explicitement masqués. */
  dossiersMasques: Set<string>;
  /** Étiquette exigée, ou `""` pour toutes. */
  tag: string;
  /** Cacher les notes sans aucun lien. */
  sansOrphelines: boolean;
  /** Centre du voisinage, ou `null` pour le graphe entier. */
  centre: string | null;
  profondeur: number;
}

export interface SousGraphe {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

/** Applique les critères, dans l'ordre qui les rend cohérents. */
export function sousGraphe(
  nodes: GraphNode[],
  edges: GraphEdge[],
  c: Criteres,
): SousGraphe {
  let retenus = nodes;
  if (c.dossiersMasques.size > 0) {
    retenus = retenus.filter((n) => !c.dossiersMasques.has(n.folder));
  }
  if (c.tag) retenus = retenus.filter((n) => n.tags.includes(c.tag));
  if (c.sansOrphelines) {
    retenus = retenus.filter((n) => n.inDegree + n.outDegree > 0);
  }

  let ids = new Set(retenus.map((n) => n.id));
  let liens = edges.filter((e) => ids.has(e.from) && ids.has(e.to));

  if (c.centre) {
    // Le voisinage se calcule sur les liens déjà filtrés : un dossier masqué
    // ne doit pas servir de pont.
    const proches = voisinage(c.centre, liens, c.profondeur);
    retenus = retenus.filter((n) => proches.has(n.id));
    ids = new Set(retenus.map((n) => n.id));
    liens = liens.filter((e) => ids.has(e.from) && ids.has(e.to));
  }

  return { nodes: retenus, edges: liens };
}

/** Liens entrants et sortants d'une note, sur le graphe **entier**. */
export function liensDe(
  id: string,
  edges: GraphEdge[],
): { sortants: string[]; entrants: string[] } {
  const sortants: string[] = [];
  const entrants: string[] = [];
  for (const e of edges) {
    if (e.from === id) sortants.push(e.to);
    if (e.to === id) entrants.push(e.from);
  }
  return { sortants, entrants };
}
