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
 * **Taille.** Le rayon suit le nombre de liens *dans le coffre entier*, jamais
 * dans la vue filtrée : ce que l'on cherche, ce sont les notes pivots, et
 * elles ne doivent pas changer de taille parce qu'on a masqué un dossier.
 *
 * **Étiquettes.** Seules les notes les plus reliées sont nommées en
 * permanence. Nommer chaque nœud rendrait l'ensemble illisible dès trente
 * notes ; le survol et la sélection donnent le reste.
 *
 * **Filtrer vs. chercher.** Masquer un dossier, exiger une étiquette ou
 * réduire au voisinage change l'ensemble affiché : le placement est refait,
 * et le sous-graphe occupe tout le cadre. Chercher, non — taper dans le champ
 * met en avant sans rien déplacer. Réarranger le graphe à chaque frappe le
 * rendrait illisible pendant qu'on tape.
 */
import { useEffect, useMemo, useRef, useState } from "react";

import * as ipc from "../lib/ipc";
import { ajusterAuCadre, cadre, placer } from "../lib/graphLayout";
import { correspond, liensDe, sousGraphe } from "../lib/graphFilter";
import type { GraphNode, VaultGraph as Graphe } from "../types/ipc";

/**
 * Nombre de nœuds nommés en permanence.
 *
 * Volontairement bas dans un panneau étroit : au-delà, les étiquettes se
 * chevauchent et l'on ne lit plus ni les noms ni la structure.
 */
const LABELS_PANNEAU = 5;
const LABELS_PLEIN_ECRAN = 14;

const RAYON_MIN = 4;
const RAYON_MAX = 13;

/** Au-delà, un déplacement du pointeur est un glissement, pas un clic. */
const SEUIL_CLIC = 4;

/** Profondeurs proposées pour le voisinage. */
const PROFONDEURS = [1, 2, 3];

interface Props {
  onOpen: (path: string) => void;
  /** Le plein écran laisse la place à plus d'étiquettes. */
  plein?: boolean;
  onPlein?: (v: boolean) => void;
}

type Glissement =
  | { quoi: "fond"; x: number; y: number; px: number; py: number }
  | { quoi: "noeud"; id: string; x: number; y: number; bouge: boolean };

export default function VaultGraph({ onOpen, plein = false, onPlein }: Props): React.JSX.Element {
  const [graphe, setGraphe] = useState<Graphe | null>(null);
  const [erreur, setErreur] = useState<string | null>(null);
  const [chargement, setChargement] = useState(true);
  const [survole, setSurvole] = useState<string | null>(null);
  const [zoom, setZoom] = useState(1);
  const [pan, setPan] = useState({ x: 0, y: 0 });
  const [casses, setCasses] = useState(false);
  const [ratio, setRatio] = useState(1);

  // Sélection et filtres.
  const [selection, setSelection] = useState<string | null>(null);
  const [recherche, setRecherche] = useState("");
  const [dossiersMasques, setDossiersMasques] = useState<Set<string>>(new Set());
  const [tag, setTag] = useState("");
  const [sansOrphelines, setSansOrphelines] = useState(false);
  const [centre, setCentre] = useState<string | null>(null);
  const [profondeur, setProfondeur] = useState(1);
  /**
   * Nœuds épinglés à la main, en unités du repère.
   *
   * Les positions sont marquées de la vue dans laquelle elles ont été posées.
   * Changer de filtre ou de cadrage refait le placement : les coordonnées
   * d'avant n'y désignent plus rien, et les épingles cessent d'elles-mêmes de
   * s'appliquer. Les relâcher depuis un effet aurait coûté un rendu de plus à
   * chaque changement de filtre.
   */
  const [epingles, setEpingles] = useState<{
    cle: string;
    pos: Record<string, { x: number; y: number }>;
  }>({ cle: "", pos: {} });

  const glisse = useRef<Glissement | null>(null);
  const zone = useRef<HTMLDivElement | null>(null);
  const svgRef = useRef<SVGSVGElement | null>(null);

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

  const vu = useMemo(
    () =>
      graphe
        ? sousGraphe(graphe.nodes, graphe.edges, {
            dossiersMasques,
            tag,
            sansOrphelines,
            centre,
            profondeur,
          })
        : { nodes: [], edges: [] },
    [graphe, dossiersMasques, tag, sansOrphelines, centre, profondeur],
  );

  // Le placement est coûteux et déterministe : on ne le refait que si
  // l'ensemble affiché change réellement, pas à chaque survol ni à chaque
  // frappe dans la recherche. Étant déterministe, retirer un filtre redonne
  // exactement le dessin d'avant.
  const places = useMemo(
    () => ajusterAuCadre(placer(vu.nodes, vu.edges, ratio), ratio),
    [vu, ratio],
  );

  /** Identifie le placement courant : ce qui périme les épingles. */
  const cleVue = useMemo(
    () =>
      [
        [...dossiersMasques].sort().join(","),
        tag,
        sansOrphelines,
        centre ?? "",
        profondeur,
        ratio.toFixed(3),
      ].join("|"),
    [dossiersMasques, tag, sansOrphelines, centre, profondeur, ratio],
  );

  const parId = useMemo(() => {
    const pos = epingles.cle === cleVue ? epingles.pos : {};
    const m = new Map<string, { id: string; x: number; y: number }>();
    for (const p of places) {
      const e = pos[p.id];
      m.set(p.id, e ? { id: p.id, x: e.x, y: e.y } : p);
    }
    return m;
  }, [places, epingles, cleVue]);

  const noeudParId = useMemo(
    () => new Map((graphe?.nodes ?? []).map((n) => [n.id, n])),
    [graphe],
  );

  const vue = useMemo(() => cadre([...parId.values()]), [parId]);

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

  /** Tous les dossiers, colorés d'abord : la légende sert aussi de filtre. */
  const dossiers = useMemo(() => {
    if (!graphe) return [];
    const tous = [...new Set(graphe.nodes.map((n) => n.folder))].sort();
    return [
      ...dossiersColores,
      ...tous.filter((d) => !dossiersColores.includes(d)),
    ];
  }, [graphe, dossiersColores]);

  const couleur = (n: GraphNode): string => {
    const i = dossiersColores.indexOf(n.folder);
    return i >= 0 ? `var(--series-${i + 1})` : "none";
  };

  const degre = (n: GraphNode): number => n.inDegree + n.outDegree;

  // Le rayon se calcule sur le coffre entier : masquer un dossier ne doit pas
  // faire grossir ou maigrir les notes restantes.
  const rayon = useMemo(() => {
    const max = Math.max(1, ...(graphe?.nodes.map(degre) ?? [1]));
    return (n: GraphNode): number =>
      RAYON_MIN + (RAYON_MAX - RAYON_MIN) * Math.sqrt(degre(n) / max);
  }, [graphe]);

  /** Notes nommées en permanence : les plus reliées de la vue. */
  const nommes = useMemo(() => {
    return new Set(
      [...vu.nodes]
        .sort((a, b) => degre(b) - degre(a))
        .slice(0, plein ? LABELS_PLEIN_ECRAN : LABELS_PANNEAU)
        .map((n) => n.id),
    );
  }, [vu, plein]);

  /**
   * Sélection effective : une note masquée par un filtre ne peut pas rester
   * choisie. Sans cela la carte décrit une note absente du dessin, et le
   * voisinage estompe tout le reste puisque rien n'est relié à l'invisible.
   */
  const choisi = useMemo(() => {
    if (!selection) return null;
    return vu.nodes.some((n) => n.id === selection) ? selection : null;
  }, [selection, vu]);

  /**
   * Notes mises en avant, le reste étant estompé.
   *
   * La recherche l'emporte sur le survol : on cherche pour retrouver, et un
   * passage de souris ne doit pas effacer le résultat.
   */
  const enAvant = useMemo(() => {
    if (recherche.trim()) {
      return new Set(vu.nodes.filter((n) => correspond(n, recherche)).map((n) => n.id));
    }
    const pivot = survole ?? choisi;
    if (!pivot) return null;
    const set = new Set<string>([pivot]);
    for (const a of vu.edges) {
      if (a.from === pivot) set.add(a.to);
      if (a.to === pivot) set.add(a.from);
    }
    return set;
  }, [recherche, survole, choisi, vu]);

  const trouvees = useMemo(
    () =>
      recherche.trim()
        ? vu.nodes.filter((n) => correspond(n, recherche)).length
        : null,
    [recherche, vu],
  );

  /** Liens de la note choisie, sur le coffre entier : un lien masqué par un
   * filtre reste un lien de cette note. */
  const liens = useMemo(
    () => (choisi && graphe ? liensDe(choisi, graphe.edges) : null),
    [choisi, graphe],
  );

  const basculerDossier = (d: string): void => {
    setDossiersMasques((prev) => {
      const s = new Set(prev);
      if (s.has(d)) s.delete(d);
      else s.add(d);
      return s;
    });
  };

  const toutMontrer = (): void => {
    setDossiersMasques(new Set());
    setTag("");
    setSansOrphelines(false);
    setCentre(null);
    setRecherche("");
  };

  const filtre =
    dossiersMasques.size > 0 || tag !== "" || sansOrphelines || centre !== null;

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

  /** Pixels écran → unités du repère, pour convertir un geste. */
  const parPixel = (): number => largeur / (svgRef.current?.clientWidth || 1);

  const selectionne = choisi ? noeudParId.get(choisi) : null;

  return (
    <div className="graph-wrap" ref={zone}>
      <div className="graph-toolbar">
        <span className="runtime-meta">
          {vu.nodes.length === graphe.nodes.length
            ? `${graphe.nodes.length} notes · ${graphe.edges.length} liens`
            : `${vu.nodes.length}/${graphe.nodes.length} notes · ${vu.edges.length} liens`}
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
            setEpingles({ cle: "", pos: {} });
          }}
          title="Recadrer et relâcher les nœuds déplacés"
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

      <div className="graph-filtres">
        <input
          type="search"
          className="graph-recherche"
          value={recherche}
          placeholder="Chercher une note, une étiquette…"
          onChange={(e) => setRecherche(e.target.value)}
          aria-label="Chercher dans le graphe"
        />
        {trouvees !== null && (
          <span className="runtime-meta">{trouvees} trouvée(s)</span>
        )}
        {graphe.tags.length > 0 && (
          <select
            className="graph-select"
            value={tag}
            onChange={(e) => setTag(e.target.value)}
            aria-label="Filtrer par étiquette"
          >
            <option value="">toutes étiquettes</option>
            {graphe.tags.map((t) => (
              <option key={t} value={t}>
                #{t}
              </option>
            ))}
          </select>
        )}
        <label className="graph-case">
          <input
            type="checkbox"
            checked={sansOrphelines}
            onChange={(e) => setSansOrphelines(e.target.checked)}
          />
          sans orphelines
        </label>
        {centre && (
          <span className="graph-focus">
            voisinage de <strong>{noeudParId.get(centre)?.name ?? centre}</strong>
            <select
              className="graph-select"
              value={profondeur}
              onChange={(e) => setProfondeur(Number(e.target.value))}
              aria-label="Profondeur du voisinage"
            >
              {PROFONDEURS.map((p) => (
                <option key={p} value={p}>
                  {p} lien{p > 1 ? "s" : ""}
                </option>
              ))}
            </select>
          </span>
        )}
        {filtre && (
          <button type="button" className="link" onClick={toutMontrer}>
            tout montrer
          </button>
        )}
      </div>

      {/* La légende est aussi le filtre par dossier : elle reste donc toujours
          présente — l'identité ne doit jamais reposer sur la seule couleur —
          tout en évitant une deuxième liste des mêmes noms. */}
      <div className="graph-legend">
        {dossiers.map((d) => {
          const i = dossiersColores.indexOf(d);
          const masque = dossiersMasques.has(d);
          return (
            <button
              type="button"
              className={`graph-legend-item ${masque ? "masque" : ""}`}
              key={d}
              onClick={() => basculerDossier(d)}
              aria-pressed={!masque}
              title={masque ? `Montrer ${d}` : `Masquer ${d}`}
            >
              <span
                className={i >= 0 ? "graph-swatch" : "graph-swatch graph-swatch-other"}
                style={i >= 0 ? { background: `var(--series-${i + 1})` } : undefined}
                aria-hidden="true"
              />
              {d}
            </button>
          );
        })}
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

      <div className="graph-scene">
        <svg
          ref={svgRef}
          className="graph-svg"
          viewBox={viewBox}
          role="img"
          aria-label={`Graphe du coffre : ${vu.nodes.length} notes reliées par ${vu.edges.length} liens`}
          onWheel={(e) => {
            const facteur = e.deltaY < 0 ? 1.15 : 1 / 1.15;
            setZoom((z) => Math.min(8, Math.max(0.4, z * facteur)));
          }}
          onPointerDown={(e) => {
            // Le fond seulement : un pointerdown sur un nœud est intercepté
            // par le nœud, qui décide s'il s'agit d'un déplacement.
            glisse.current = {
              quoi: "fond",
              x: e.clientX,
              y: e.clientY,
              px: pan.x,
              py: pan.y,
            };
            e.currentTarget.setPointerCapture(e.pointerId);
          }}
          onPointerMove={(e) => {
            const g = glisse.current;
            if (!g) return;
            const k = parPixel();
            if (g.quoi === "fond") {
              // À fort zoom, un même geste doit parcourir moins de graphe.
              setPan({
                x: g.px - (e.clientX - g.x) * k,
                y: g.py - (e.clientY - g.y) * k,
              });
              return;
            }
            const dx = e.clientX - g.x;
            const dy = e.clientY - g.y;
            if (!g.bouge && Math.hypot(dx, dy) < SEUIL_CLIC) return;
            g.bouge = true;
            const base = parId.get(g.id);
            if (!base) return;
            g.x = e.clientX;
            g.y = e.clientY;
            setEpingles((prev) => ({
              cle: cleVue,
              pos: {
                ...(prev.cle === cleVue ? prev.pos : {}),
                [g.id]: { x: base.x + dx * k, y: base.y + dy * k },
              },
            }));
          }}
          onPointerUp={(e) => {
            const g = glisse.current;
            glisse.current = null;
            if (e.currentTarget.hasPointerCapture(e.pointerId)) {
              e.currentTarget.releasePointerCapture(e.pointerId);
            }
            // Un glissement de quelques pixels reste un clic : sans ce seuil,
            // sélectionner un nœud à la souris échoue une fois sur deux.
            if (g?.quoi === "noeud" && !g.bouge) setSelection(g.id);
          }}
        >
          <g className="graph-edges">
            {vu.edges.map((a, i) => {
              const d = parId.get(a.from);
              const f = parId.get(a.to);
              if (!d || !f) return null;
              const actif = !enAvant || (enAvant.has(a.from) && enAvant.has(a.to));
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
            {vu.nodes.map((n) => {
              const p = parId.get(n.id);
              if (!p) return null;
              const r = rayon(n) * echelle;
              const estime = !enAvant || enAvant.has(n.id);
              const remplissage = couleur(n);
              const classes = [
                "graph-node",
                estime ? "" : "muted",
                choisi === n.id ? "choisi" : "",
              ]
                .filter(Boolean)
                .join(" ");
              return (
                <g
                  key={n.id}
                  className={classes}
                  onPointerEnter={() => setSurvole(n.id)}
                  onPointerLeave={() => setSurvole(null)}
                  onPointerDown={(e) => {
                    e.stopPropagation();
                    glisse.current = {
                      quoi: "noeud",
                      id: n.id,
                      x: e.clientX,
                      y: e.clientY,
                      bouge: false,
                    };
                    svgRef.current?.setPointerCapture(e.pointerId);
                  }}
                  role="button"
                  tabIndex={0}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") setSelection(n.id);
                    if (e.key === " ") {
                      e.preventDefault();
                      onOpen(n.id);
                    }
                  }}
                >
                  <title>
                    {`${n.name}\n${n.folder} · ${n.inDegree} entrant(s), ${n.outDegree} sortant(s)${
                      n.tags.length ? `\n#${n.tags.join(" #")}` : ""
                    }`}
                  </title>
                  {choisi === n.id && (
                    <circle
                      cx={p.x}
                      cy={p.y}
                      r={r + 4 * echelle}
                      className="graph-halo"
                      vectorEffect="non-scaling-stroke"
                    />
                  )}
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
                  {(nommes.has(n.id) || survole === n.id || choisi === n.id) && (
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

        {selectionne && liens && (
          <aside className="graph-carte" aria-label={`Liens de ${selectionne.name}`}>
            <div className="graph-carte-tete">
              <strong>{selectionne.name}</strong>
              <button
                type="button"
                className="btn btn-mini"
                onClick={() => setSelection(null)}
                aria-label="Fermer"
              >
                ×
              </button>
            </div>
            <p className="runtime-meta">
              {selectionne.id}
              {selectionne.tags.length > 0 && ` · #${selectionne.tags.join(" #")}`}
            </p>

            <ListeLiens
              titre="Sortants"
              ids={liens.sortants}
              noms={noeudParId}
              onChoisir={setSelection}
            />
            <ListeLiens
              titre="Entrants"
              ids={liens.entrants}
              noms={noeudParId}
              onChoisir={setSelection}
            />

            <div className="team-actions">
              <button
                type="button"
                className="btn btn-mini btn-primary"
                onClick={() => onOpen(selectionne.id)}
              >
                Ouvrir
              </button>
              <button
                type="button"
                className="btn btn-mini"
                onClick={() => setCentre(centre === selectionne.id ? null : selectionne.id)}
              >
                {centre === selectionne.id ? "Tout le coffre" : "Voisinage"}
              </button>
            </div>
          </aside>
        )}
      </div>
    </div>
  );
}

/** Une colonne de liens cliquables, ou rien du tout s'il n'y en a aucun. */
function ListeLiens({
  titre,
  ids,
  noms,
  onChoisir,
}: {
  titre: string;
  ids: string[];
  noms: Map<string, GraphNode>;
  onChoisir: (id: string) => void;
}): React.JSX.Element {
  if (ids.length === 0) {
    return <p className="runtime-meta">{titre} : aucun</p>;
  }
  return (
    <div className="graph-liens">
      <span className="runtime-meta">
        {titre} ({ids.length})
      </span>
      <ul>
        {ids.map((id) => (
          <li key={id}>
            <button type="button" className="link" onClick={() => onChoisir(id)}>
              {noms.get(id)?.name ?? id}
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
