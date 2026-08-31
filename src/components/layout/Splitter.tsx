/**
 * Poignée de redimensionnement entre deux zones.
 *
 * Le glissement passe par les événements *pointer* et non *mouse* : la capture
 * de pointeur garantit que le geste continue même si le curseur sort de la
 * poignée, ce qui arrive constamment quand on tire vite.
 *
 * La poignée reste atteignable au clavier (flèches, Origine/Fin) — sans quoi
 * un utilisateur qui n'emploie pas la souris ne pourrait plus régler la
 * largeur du tout.
 */
import { useRef, type KeyboardEvent, type PointerEvent } from "react";

import { LARGEUR_MAX, LARGEUR_MIN } from "../../hooks/useLayout";

interface Props {
  /** Largeur actuelle de la zone ajustée, en pixels. */
  value: number;
  onChange: (largeur: number) => void;
  /** Largeur rétablie au double-clic. */
  defaultValue: number;
  /**
   * `1` quand la zone est à gauche de la poignée (tirer à droite élargit),
   * `-1` quand elle est à droite.
   */
  sign: 1 | -1;
  label: string;
}

const PAS_CLAVIER = 16;

export default function Splitter({
  value,
  onChange,
  defaultValue,
  sign,
  label,
}: Props): React.JSX.Element {
  const depart = useRef<{ x: number; largeur: number } | null>(null);

  const onPointerDown = (e: PointerEvent<HTMLDivElement>): void => {
    depart.current = { x: e.clientX, largeur: value };
    e.currentTarget.setPointerCapture(e.pointerId);
  };

  const onPointerMove = (e: PointerEvent<HTMLDivElement>): void => {
    if (!depart.current) return;
    const delta = (e.clientX - depart.current.x) * sign;
    onChange(depart.current.largeur + delta);
  };

  const onPointerUp = (e: PointerEvent<HTMLDivElement>): void => {
    depart.current = null;
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>): void => {
    const touches: Record<string, number> = {
      ArrowLeft: -PAS_CLAVIER * sign,
      ArrowRight: PAS_CLAVIER * sign,
    };
    if (e.key in touches) {
      e.preventDefault();
      onChange(value + touches[e.key]);
    } else if (e.key === "Home") {
      e.preventDefault();
      onChange(LARGEUR_MIN);
    } else if (e.key === "End") {
      e.preventDefault();
      onChange(LARGEUR_MAX);
    }
  };

  return (
    <div
      className="splitter"
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      aria-valuenow={Math.round(value)}
      aria-valuemin={LARGEUR_MIN}
      aria-valuemax={LARGEUR_MAX}
      tabIndex={0}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onDoubleClick={() => onChange(defaultValue)}
      onKeyDown={onKeyDown}
      title="Glisser pour redimensionner — double-clic pour rétablir"
    >
      <span className="splitter-grip" aria-hidden="true" />
    </div>
  );
}
