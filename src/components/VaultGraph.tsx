/**
 * Vue graphique du coffre.
 *
 * Reconstruite depuis les fichiers Markdown, pas obtenue d'Obsidian : il
 * n'expose aucune API externe, et un greffon communautaire exigerait qu'il
 * tourne sans fournir pour autant le graphe déjà dessiné.
 *
 * **Couleur.** Les nœuds d'un graphe se côtoient tous, c'est le cas « toutes
 * paires » : il plafonne à trois catégories validées. Les trois dossiers les
 * plus fournis reçoivent donc un emplacement chacun ; les autres ne prennent
 * pas une quatrième teinte mais un cercle non rempli — une distinction de
 * forme, pas de couleur, qui ne dégrade aucune paire.
 *
 * **Taille.** Le rayon suit le nombre de liens : ce que l'on cherche dans un
 * graphe de coffre, ce sont les notes pivots.
 *
 * **Étiquettes.** Seules les notes les plus reliées sont nommées en
 * permanence. Nommer chaque nœud rendrait l'ensemble illisible dès trente
 * notes ; le survol donne le reste.
 */
import { useEffect, useMemo, useRef, useState } from "react";

import * as ipc from "../lib/ipc";
import { ajusterAuCadre, cadre, placer } from "../lib/graphLayout";
import type { GraphNode, VaultGraph as Graphe } from "../types/ipc";

/**
 * Nombre de nœuds nommés en permanence.
 *
 * Volontairement bas dans un panneau étroit : au-delà, les étiquettes se
 * chevauchent et l'on ne lit plus ni les noms ni la structure. Le survol
 * donne le reste.
 */
const LABELS_PANNEAU = 5;
const LABELS_PLEIN_ECRAN = 14;

const RAYON_MIN = 4;
const RAYON_MAX = 13;

interface Props {
  onOpen: (path: string) => void;
  /** Le plein écran laisse la place à plus d'étiquettes. */
  plein?: boolean;
  onPlein?: (v: boolean) => void;
}

export default function VaultGraph({ onOpen, plein = false, onPlein }: Props): React.JSX.Element {
  const [graphe, setGraphe] = useState<Graphe | null>(null);
  const [erreur, setErreur] = useState<string | null>(null);
  const [chargement, setChargement] = useState(true);
  const [survole, setSurvole] = useState<string | null>(null);
  const [zoom, setZoom] = useState(1);
  const [pan, setPan] = useState({ x: 0, y: 0 });
  const [casses, setCasses] = useState(false);
  const glisse = useRef<{ x: number; y: number; px: number; py: number } | null>(null);
  const zone = useRef<HTMLDivElement | null>(null);
  const [ratio, setRatio] = useState(1);

  // Le placement suit la forme de la zone : un graphe carré dans un panneau
  // étroit et haut serait réduit pour tenir en largeur, laissant d'immenses
  // marges verticales.
  useEffect(() => {
    const el = zone.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const obs = new ResizeObserver(([entree]) => {
      const { width, height } = entree.contentRect;
      if (width > 0 && height > 0) setRatio(width / height);
    });
    obs.observe(el);
    return () => obs.disconnect();
  }, []);

  const charger = async (): Promise<void> => {
    setChargement(true);
    try {
      setGraphe(await ipc.vaultGraph());
      setErreur(null);
    } catch (e) {
      setErreur(e instanceof Error ? e.message : String(e));
    } finally {
      setChargement(false);
    }
  };

  useEffect(() => {
    void charger();
  }, []);

  // Le placement est coûteux et déterministe : on ne le refait que si le
  // graphe change réellement, pas à chaque survol.
  const places = useMemo(
    () =>
      graphe ? ajusterAuCadre(placer(graphe.nodes, graphe.edges, ratio), ratio) : [],
    [graphe, ratio],
  );
  const parId = useMemo(() => new Map(places.map((p) => [p.id, p])), [places]);
  const vue = useMemo(() => cadre(places), [places]);

  /** Les trois dossiers les plus fournis reçoivent une couleur. */
  const dossiersColores = useMemo(() => {
    if (!graphe) return [];
    const compte = new Map<string, number>();
    for (const n of graphe.nodes) {
      compte.set(n.folder, (compte.get(n.folder) ?? 0) + 1);
    }
    return [...compte.entries()]
      .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
      .slice(0, 3)
      .map(([nom]) => nom);
  }, [graphe]);

  const couleur = (n: GraphNode): string => {
    const i = dossiersColores.indexOf(n.folder);
    return i >= 0 ? `var(--series-${i + 1})` : "none";
  };

  const degre = (n: GraphNode): number => n.inDegree + n.outDegree;

  const rayon = useMemo(() => {
    const max = Math.max(1, ...(graphe?.nodes.map(degre) ?? [1]));
    return (n: GraphNode): number =>
      RAYON_MIN + (RAYON_MAX - RAYON_MIN) * Math.sqrt(degre(n) / max);
  }, [graphe]);

  /** Notes nommées en permanence : les plus reliées. */
  const nommes = useMemo(() => {
    if (!graphe) return new Set<string>();
    return new Set(
      [...graphe.nodes]
        .sort((a, b) => degre(b) - degre(a))
        .slice(0, plein ? LABELS_PLEIN_ECRAN : LABELS_PANNEAU)
        .map((n) => n.id),
    );
  }, [graphe, plein]);

  /** Voisinage du nœud survolé, pour estomper le reste. */
  const voisins = useMemo(() => {
    if (!survole || !graphe) return null;
    const set = new Set<string>([survole]);
    for (const a of graphe.edges) {
      if (a.from === survole) set.add(a.to);
      if (a.to === survole) set.add(a.from);
    }
    return set;
  }, [survole, graphe]);

  if (chargement) return <p className="empty-hint">Lecture du coffre…</p>;
  if (erreur) return <p className="err-text">{erreur}</p>;
  if (!graphe || graphe.nodes.length === 0) {
    return <p className="empty-hint">Aucune note dans le coffre.</p>;
  }

  const largeur = vue.w / zoom;
  const hauteur = vue.h / zoom;
  const viewBox = `${vue.x + pan.x} ${vue.y + pan.y} ${largeur} ${hauteur}`;

  /**
   * Facteur de conversion pixels → unités du repère.
   *
   * Rayons et étiquettes sont exprimés en unités du repère : sans ce facteur,
   * un graphe resserré — donc au cadre étroit — afficherait des nœuds énormes
   * et des noms illisibles par-dessus tout le reste. On les ramène à une
   * taille constante à l'écran, zoom compris.
   */
  const echelle = largeur / 900;

  return (
    <div className="graph-wrap" ref={zone}>
      <div className="graph-toolbar">
        <span className="runtime-meta">
          {graphe.nodes.length} notes · {graphe.edges.length} liens
        </span>
        {graphe.broken.length > 0 && (
          <button
            type="button"
            className="link"
            onClick={() => setCasses((v) => !v)}
            title="Liens vers des notes inexistantes"
          >
            {graphe.broken.length} lien(s) cassé(s)
          </button>
        )}
        <button type="button" className="btn btn-mini" onClick={() => void charger()}>
          Relire
        </button>
        <button
          type="button"
          className="btn btn-mini"
          onClick={() => {
            setZoom(1);
            setPan({ x: 0, y: 0 });
          }}
          title="Recadrer"
        >
          ⌖
        </button>
        {onPlein && (
          <button
            type="button"
            className="btn btn-mini"
            onClick={() => onPlein(!plein)}
            title={plein ? "Revenir au panneau" : "Agrandir"}
          >
            {plein ? "⤡" : "⤢"}
          </button>
        )}
      </div>

      {/* Une légende est présente dès deux catégories : l'identité ne doit
          jamais reposer sur la seule couleur. */}
      <div className="graph-legend">
        {dossiersColores.map((d, i) => (
          <span className="graph-legend-item" key={d}>
            <span
              className="graph-swatch"
              style={{ background: `var(--series-${i + 1})` }}
              aria-hidden="true"
            />
            {d}
          </span>
        ))}
        <span className="graph-legend-item">
          <span className="graph-swatch graph-swatch-other" aria-hidden="true" />
          autres
        </span>
      </div>

      {casses && (
        <ul className="graph-broken">
          {graphe.broken.slice(0, 40).map((b, i) => (
            <li key={i} className="runtime-meta">
              {b.from} → <strong>{b.target}</strong>
            </li>
          ))}
        </ul>
      )}

      <svg
        className="graph-svg"
        viewBox={viewBox}
        role="img"
        aria-label={`Graphe du coffre : ${graphe.nodes.length} notes reliées par ${graphe.edges.length} liens`}
        onWheel={(e) => {
          const facteur = e.deltaY < 0 ? 1.15 : 1 / 1.15;
          setZoom((z) => Math.min(8, Math.max(0.4, z * facteur)));
        }}
        onPointerDown={(e) => {
          glisse.current = { x: e.clientX, y: e.clientY, px: pan.x, py: pan.y };
          e.currentTarget.setPointerCapture(e.pointerId);
        }}
        onPointerMove={(e) => {
          if (!glisse.current) return;
          // Le déplacement est converti en unités du repère : à fort zoom, un
          // même geste doit parcourir moins de graphe.
          const echelle = largeur / e.currentTarget.clientWidth;
          setPan({
            x: glisse.current.px - (e.clientX - glisse.current.x) * echelle,
            y: glisse.current.py - (e.clientY - glisse.current.y) * echelle,
          });
        }}
        onPointerUp={(e) => {
          glisse.current = null;
          if (e.currentTarget.hasPointerCapture(e.pointerId)) {
            e.currentTarget.releasePointerCapture(e.pointerId);
          }
        }}
      >
        <g className="graph-edges">
          {graphe.edges.map((a, i) => {
            const d = parId.get(a.from);
            const f = parId.get(a.to);
            if (!d || !f) return null;
            const actif = !voisins || (voisins.has(a.from) && voisins.has(a.to));
            return (
              <line
                key={i}
                x1={d.x}
                y1={d.y}
                x2={f.x}
                y2={f.y}
                className={actif ? "graph-edge" : "graph-edge muted"}
                vectorEffect="non-scaling-stroke"
              />
            );
          })}
        </g>

        <g className="graph-nodes">
          {graphe.nodes.map((n) => {
            const p = parId.get(n.id);
            if (!p) return null;
            const r = rayon(n) * echelle;
            const estime = !voisins || voisins.has(n.id);
            const remplissage = couleur(n);
            return (
              <g
                key={n.id}
                className={`graph-node ${estime ? "" : "muted"}`}
                onPointerEnter={() => setSurvole(n.id)}
                onPointerLeave={() => setSurvole(null)}
                onClick={() => onOpen(n.id)}
                role="button"
                tabIndex={0}
                onKeyDown={(e) => {
                  if (e.key === "Enter") onOpen(n.id);
                }}
              >
                <title>
                  {`${n.name}\n${n.folder} · ${n.inDegree} entrant(s), ${n.outDegree} sortant(s)${
                    n.tags.length ? `\n#${n.tags.join(" #")}` : ""
                  }`}
                </title>
                <circle
                  cx={p.x}
                  cy={p.y}
                  r={r}
                  fill={remplissage}
                  // Le cercle non rempli distingue les autres dossiers sans
                  // introduire une quatrième teinte.
                  className={remplissage === "none" ? "graph-dot other" : "graph-dot"}
                  vectorEffect="non-scaling-stroke"
                />
                {(nommes.has(n.id) || survole === n.id) && (
                  <text
                    x={p.x}
                    y={p.y - r - 5 * echelle}
                    className="graph-label"
                    fontSize={11 * echelle}
                    strokeWidth={3 * echelle}
                  >
                    {n.name}
                  </text>
                )}
              </g>
            );
          })}
        </g>
      </svg>
    </div>
  );
}
