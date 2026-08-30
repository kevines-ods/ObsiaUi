/**
 * Point d'accroche des plugins.
 *
 * Chaque montage reçoit son propre conteneur : deux plugins accrochés au même
 * endroit ne se marchent pas dessus, et le nettoyage rendu par un plugin est
 * appelé quand la zone disparaît.
 */
import { useEffect, useRef, useState } from "react";

import { montagesPour, surChangement } from "../lib/plugins";
import type { MountPoint } from "../types/ipc";

export default function PluginSlot({ point }: { point: MountPoint }): React.JSX.Element | null {
  const [version, setVersion] = useState(0);
  const conteneurs = useRef(new Map<string, HTMLDivElement | null>());

  // Un rechargement de plugins doit refaire le rendu de la zone.
  useEffect(() => surChangement(() => setVersion((v) => v + 1)), []);

  const montages = montagesPour(point);

  useEffect(() => {
    const nettoyages: Array<() => void> = [];
    for (const [i, montage] of montages.entries()) {
      const el = conteneurs.current.get(`${montage.pluginId}-${i}`);
      if (!el) continue;
      el.replaceChildren();
      try {
        const nettoyage = montage.render(el);
        if (typeof nettoyage === "function") nettoyages.push(nettoyage);
      } catch (e) {
        // Un plugin qui échoue au rendu ne doit pas vider la zone entière.
        el.textContent = `${montage.pluginName} : ${
          e instanceof Error ? e.message : String(e)
        }`;
      }
    }
    return () => {
      for (const n of nettoyages) {
        try {
          n();
        } catch {
          // Un nettoyage fautif ne doit pas empêcher les suivants.
        }
      }
    };
  }, [montages, version]);

  if (montages.length === 0) return null;

  return (
    <div className={`plugin-slot plugin-slot-${point}`}>
      {montages.map((m, i) => (
        <div
          key={`${m.pluginId}-${i}`}
          className="plugin-mount"
          data-plugin={m.pluginId}
          ref={(el) => {
            conteneurs.current.set(`${m.pluginId}-${i}`, el);
          }}
        />
      ))}
    </div>
  );
}
