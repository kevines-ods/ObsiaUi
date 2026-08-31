/**
 * Disposition des trois zones : ouverture et largeurs.
 *
 * Conservée dans le stockage local et non dans la configuration : c'est un
 * réglage propre au poste et à son écran, pas une préférence à transporter
 * d'une machine à l'autre. Un patch d'interface peut en fixer les valeurs de
 * départ (`LayoutPatch`), l'ajustement à la souris prend ensuite le dessus.
 */
import { useCallback, useEffect, useState } from "react";

const CLE = "obsia.layout";

/** Bornes de largeur, alignées sur celles que le backend valide. */
export const LARGEUR_MIN = 180;
export const LARGEUR_MAX = 900;

export interface Layout {
  leftOpen: boolean;
  rightOpen: boolean;
  leftWidth: number;
  rightWidth: number;
}

export const LAYOUT_DEFAUT: Layout = {
  leftOpen: true,
  rightOpen: true,
  leftWidth: 300,
  rightWidth: 340,
};

export function borner(px: number): number {
  return Math.min(LARGEUR_MAX, Math.max(LARGEUR_MIN, Math.round(px)));
}

/**
 * Relit une disposition stockée en écartant ce qui n'est pas exploitable.
 *
 * Le stockage local peut contenir n'importe quoi — version précédente, édition
 * manuelle, données d'un autre site sur le même origine. Une largeur absurde
 * rendrait une zone inutilisable sans moyen évident de revenir en arrière.
 */
export function lireLayout(brut: string | null): Layout {
  if (!brut) return LAYOUT_DEFAUT;
  try {
    const v = JSON.parse(brut) as Partial<Layout>;
    return {
      leftOpen: typeof v.leftOpen === "boolean" ? v.leftOpen : LAYOUT_DEFAUT.leftOpen,
      rightOpen: typeof v.rightOpen === "boolean" ? v.rightOpen : LAYOUT_DEFAUT.rightOpen,
      leftWidth:
        typeof v.leftWidth === "number" && Number.isFinite(v.leftWidth)
          ? borner(v.leftWidth)
          : LAYOUT_DEFAUT.leftWidth,
      rightWidth:
        typeof v.rightWidth === "number" && Number.isFinite(v.rightWidth)
          ? borner(v.rightWidth)
          : LAYOUT_DEFAUT.rightWidth,
    };
  } catch {
    return LAYOUT_DEFAUT;
  }
}

export function useLayout(): {
  layout: Layout;
  setLayout: (patch: Partial<Layout>) => void;
  reset: () => void;
} {
  const [layout, setEtat] = useState<Layout>(() => {
    try {
      return lireLayout(localStorage.getItem(CLE));
    } catch {
      // Fenêtre privée ou stockage refusé : les défauts suffisent.
      return LAYOUT_DEFAUT;
    }
  });

  useEffect(() => {
    try {
      localStorage.setItem(CLE, JSON.stringify(layout));
    } catch {
      // L'échec d'écriture ne doit pas empêcher de redimensionner.
    }
  }, [layout]);

  const setLayout = useCallback((patch: Partial<Layout>): void => {
    setEtat((prev) => {
      const suivant = { ...prev, ...patch };
      return {
        ...suivant,
        leftWidth: borner(suivant.leftWidth),
        rightWidth: borner(suivant.rightWidth),
      };
    });
  }, []);

  const reset = useCallback((): void => setEtat(LAYOUT_DEFAUT), []);

  return { layout, setLayout, reset };
}
