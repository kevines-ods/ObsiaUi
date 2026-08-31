/**
 * Placement des nœuds du graphe (Fruchterman-Reingold).
 *
 * Deux propriétés comptent plus que l'esthétique :
 *
 * - **Déterminisme.** Les positions de départ dérivent de l'identifiant du
 *   nœud, pas d'un tirage aléatoire. Sans cela, le graphe se réorganiserait
 *   entièrement à chaque rafraîchissement et l'on perdrait le repère mental
 *   qu'on venait de se construire.
 * - **Coût borné.** La répulsion est en O(n²) : au-delà d'un certain nombre de
 *   notes, on réduit le nombre d'itérations plutôt que de figer la fenêtre.
 */

export interface NoeudPlace {
  id: string;
  x: number;
  y: number;
}

interface Arete {
  from: string;
  to: string;
}

/** Largeur du cadre de placement, en unités arbitraires. */
const COTE = 1000;

/** Au-delà, on réduit les itérations : la répulsion est quadratique. */
const SEUIL_GROS_GRAPHE = 250;

/**
 * Position de départ dérivée de l'identifiant.
 *
 * Un simple hachage réparti sur un disque : deux notes voisines dans l'ordre
 * alphabétique ne partent pas du même point, et le résultat est reproductible.
 */
export function positionInitiale(
  id: string,
  index: number,
  total: number,
  hauteur: number = COTE,
): { x: number; y: number } {
  let h = 2166136261;
  for (let i = 0; i < id.length; i += 1) {
    h ^= id.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  const angle = (index / Math.max(1, total)) * Math.PI * 2;
  const facteur = 0.25 + ((h >>> 0) % 1000) / 1000 / 1.4;
  return {
    x: COTE / 2 + Math.cos(angle) * (COTE / 2) * facteur,
    y: hauteur / 2 + Math.sin(angle) * (hauteur / 2) * facteur,
  };
}

/** Nombre d'itérations, réduit sur les gros graphes. */
export function iterations(nbNoeuds: number): number {
  if (nbNoeuds <= 60) return 300;
  if (nbNoeuds <= SEUIL_GROS_GRAPHE) return 180;
  return 90;
}

/**
 * Calcule les positions. Fonction pure : mêmes entrées, mêmes sorties.
 */
export function placer(
  noeuds: Array<{ id: string }>,
  aretes: Arete[],
  /**
   * Rapport largeur/hauteur de la zone d'affichage.
   *
   * Sans lui, un graphe carré affiché dans un panneau étroit et haut est
   * réduit pour tenir en largeur, laissant d'immenses marges en haut et en
   * bas. On place donc dans un cadre de même forme que la zone.
   */
  ratio = 1,
): NoeudPlace[] {
  const hauteur = COTE / Math.max(0.2, Math.min(5, ratio));
  if (noeuds.length === 0) return [];
  if (noeuds.length === 1) {
    return [{ id: noeuds[0].id, x: COTE / 2, y: hauteur / 2 }];
  }

  // Les notes sans aucun lien sortent de la simulation. Le rappel vers le
  // centre n'est pas borné par la température alors que la répulsion l'est :
  // en fin de course elles finissaient toutes empilées au même point, noms
  // superposés et illisibles. Elles vont sur un anneau autour du reste, ce
  // qui est aussi leur place logique — une orpheline n'appartient à aucun
  // groupe.
  const relie = new Set<string>();
  for (const a of aretes) {
    relie.add(a.from);
    relie.add(a.to);
  }
  const connectes = noeuds.filter((x) => relie.has(x.id));
  const isoles = noeuds.filter((x) => !relie.has(x.id));

  // Un coffre sans aucun lien : tout est orphelin, l'anneau porte tout.
  if (connectes.length === 0) {
    return anneau(isoles, COTE / 2, hauteur / 2, COTE * 0.4, hauteur * 0.4);
  }

  const n = connectes.length;
  const pos = connectes.map((noeud, i) => ({
    id: noeud.id,
    ...positionInitiale(noeud.id, i, n, hauteur),
  }));
  const index = new Map(pos.map((p, i) => [p.id, i]));

  // Distance idéale entre deux nœuds : la surface disponible par nœud.
  const k = Math.sqrt((COTE * hauteur) / n);
  const total = iterations(n);
  let temperature = COTE / 8;

  for (let pas = 0; pas < total; pas += 1) {
    const dx = new Float64Array(n);
    const dy = new Float64Array(n);

    // Répulsion entre toutes les paires.
    for (let i = 0; i < n; i += 1) {
      for (let j = i + 1; j < n; j += 1) {
        let ex = pos[i].x - pos[j].x;
        let ey = pos[i].y - pos[j].y;
        let d2 = ex * ex + ey * ey;
        if (d2 < 0.01) {
          // Deux nœuds exactement superposés ne se repousseraient jamais :
          // on les décale d'un écart dérivé de leur rang, sans aléa.
          ex = ((i % 7) - 3) / 10 || 0.1;
          ey = ((j % 5) - 2) / 10 || 0.1;
          d2 = ex * ex + ey * ey;
        }
        const d = Math.sqrt(d2);
        const force = (k * k) / d;
        const ux = (ex / d) * force;
        const uy = (ey / d) * force;
        dx[i] += ux;
        dy[i] += uy;
        dx[j] -= ux;
        dy[j] -= uy;
      }
    }

    // Attraction le long des arêtes.
    for (const a of aretes) {
      const i = index.get(a.from);
      const j = index.get(a.to);
      if (i === undefined || j === undefined || i === j) continue;
      const ex = pos[i].x - pos[j].x;
      const ey = pos[i].y - pos[j].y;
      const d = Math.sqrt(ex * ex + ey * ey) || 0.01;
      const force = (d * d) / k;
      const ux = (ex / d) * force;
      const uy = (ey / d) * force;
      dx[i] -= ux;
      dy[i] -= uy;
      dx[j] += ux;
      dy[j] += uy;
    }

    // Déplacement borné par la température, qui décroît : les grands
    // réarrangements ont lieu tôt, les ajustements fins à la fin.
    for (let i = 0; i < n; i += 1) {
      const d = Math.sqrt(dx[i] * dx[i] + dy[i] * dy[i]) || 1;
      const pas_i = Math.min(d, temperature);
      pos[i].x += (dx[i] / d) * pas_i;
      pos[i].y += (dy[i] / d) * pas_i;
      // Une légère attraction vers le centre garde les composantes détachées
      // dans le cadre, au lieu de les laisser dériver à l'infini.
      pos[i].x += (COTE / 2 - pos[i].x) * 0.01;
      pos[i].y += (hauteur / 2 - pos[i].y) * 0.01;
    }
    temperature *= 0.95;
  }

  if (isoles.length === 0) return pos;

  // L'anneau entoure ce que la simulation a produit, avec une marge : les
  // orphelines bordent le graphe sans venir se mêler à ses groupes. Il épouse
  // la forme du nuage plutôt que d'être circulaire — un anneau rond autour
  // d'un nuage large le rendrait carré, et tout le travail d'ajustement au
  // cadre serait perdu.
  const xs = pos.map((p) => p.x);
  const ys = pos.map((p) => p.y);
  const cx = (Math.max(...xs) + Math.min(...xs)) / 2;
  const cy = (Math.max(...ys) + Math.min(...ys)) / 2;
  const rx = Math.max(Math.max(...xs) - cx, COTE / 8) * 1.25;
  const ry = Math.max(Math.max(...ys) - cy, hauteur / 8) * 1.25;
  return [...pos, ...anneau(isoles, cx, cy, rx, ry)];
}

/**
 * Répartit des nœuds sur une ellipse, dans l'ordre donné.
 *
 * Déterministe comme le reste du placement : le même coffre redonne le même
 * anneau, et l'on retrouve une orpheline là où on l'avait laissée.
 */
function anneau(
  noeuds: Array<{ id: string }>,
  cx: number,
  cy: number,
  rx: number,
  ry: number,
): NoeudPlace[] {
  const n = noeuds.length;
  return noeuds.map((noeud, i) => {
    const angle = (i / n) * Math.PI * 2;
    return {
      id: noeud.id,
      x: cx + Math.cos(angle) * rx,
      y: cy + Math.sin(angle) * ry,
    };
  });
}

/**
 * Rapproche la forme du nuage de celle de la zone d'affichage.
 *
 * Le placement par forces est isotrope : la forme finale dépend de la
 * topologie, pas du cadre. Un coffre organisé en chaîne produit un nuage
 * vertical qui, dans une fenêtre large, se retrouve compressé au centre avec
 * d'immenses marges de part et d'autre.
 *
 * On étire donc le nuage vers la forme du cadre. La distorsion est
 * **plafonnée** : dans un graphe de connaissance les distances ne portent
 * aucune grandeur — seule la topologie compte — mais au-delà d'un certain
 * étirement les groupes cessent d'être lisibles comme groupes.
 *
 * Le plafond a été relevé de 1,5 à 2,2 après mesure : un coffre en chaîne
 * produit un nuage deux fois plus haut que large, et dans une fenêtre en
 * 16/9 l'ancien plafond laissait plus de la moitié de la largeur vide. Les
 * marques, elles, ne sont jamais déformées — seules les positions le sont.
 */
export function ajusterAuCadre(
  places: NoeudPlace[],
  ratio: number,
  maxDistorsion = 2.2,
): NoeudPlace[] {
  if (places.length < 3) return places;
  const xs = places.map((p) => p.x);
  const ys = places.map((p) => p.y);
  const largeur = Math.max(...xs) - Math.min(...xs);
  const hauteur = Math.max(...ys) - Math.min(...ys);
  if (largeur < 1 || hauteur < 1) return places;

  const vise = Math.max(0.2, Math.min(5, ratio)) / (largeur / hauteur);
  const borne = Math.min(maxDistorsion, Math.max(1 / maxDistorsion, vise));
  // Réparti sur les deux axes : on étire l'un d'autant qu'on comprime
  // l'autre, ce qui laisse la surface du nuage inchangée.
  const fx = Math.sqrt(borne);
  const fy = 1 / fx;

  const cx = (Math.max(...xs) + Math.min(...xs)) / 2;
  const cy = (Math.max(...ys) + Math.min(...ys)) / 2;
  return places.map((p) => ({
    id: p.id,
    x: cx + (p.x - cx) * fx,
    y: cy + (p.y - cy) * fy,
  }));
}

/** Cadre englobant, avec une marge, pour ajuster la vue. */
export function cadre(places: NoeudPlace[]): {
  x: number;
  y: number;
  w: number;
  h: number;
} {
  if (places.length === 0) return { x: 0, y: 0, w: COTE, h: COTE };
  const xs = places.map((p) => p.x);
  const ys = places.map((p) => p.y);
  const marge = 60;
  const x = Math.min(...xs) - marge;
  const y = Math.min(...ys) - marge;
  return {
    x,
    y,
    w: Math.max(1, Math.max(...xs) + marge - x),
    h: Math.max(1, Math.max(...ys) + marge - y),
  };
}
