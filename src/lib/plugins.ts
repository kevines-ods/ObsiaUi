/**
 * Hôte des plugins.
 *
 * Un plugin est du JavaScript évalué dans le webview, à des points d'accroche
 * qu'il déclare. Il faut être exact sur ce que cela vaut :
 *
 * **Ce n'est pas un bac à sable.** Le code s'exécute dans la page, avec tout
 * ce que cela implique. Les permissions du manifeste bornent l'API qu'ObsiaUi
 * lui tend — `obsia.call` refuse une commande non accordée — mais elles
 * n'empêchent pas un plugin déterminé d'atteindre le DOM. La vraie garde est
 * en amont : un plugin est inactif tant qu'on ne l'a pas approuvé, et toute
 * modification de son fichier le redésactive.
 *
 * Les erreurs sont attrapées par plugin : un plugin fautif ne doit pas
 * emporter l'interface avec lui.
 */
import * as ipc from "./ipc";
import { call, subscribe } from "./transport";
import type { LoadedPlugin, MountPoint } from "../types/ipc";

/** Une fonction de rendu peut renvoyer son propre nettoyage. */
export type Renderer = (element: HTMLElement) => void | (() => void);

interface Montage {
  pluginId: string;
  pluginName: string;
  render: Renderer;
}

const montages = new Map<MountPoint, Montage[]>();
const abonnes = new Set<() => void>();
let erreursChargement: string[] = [];

function notifier(): void {
  for (const cb of abonnes) cb();
}

/** Montages déclarés pour un point d'accroche. */
export function montagesPour(point: MountPoint): Montage[] {
  return montages.get(point) ?? [];
}

/** Erreurs rencontrées au dernier chargement. */
export function erreurs(): string[] {
  return erreursChargement;
}

/** S'abonne aux changements de la liste des montages. */
export function surChangement(cb: () => void): () => void {
  abonnes.add(cb);
  return () => abonnes.delete(cb);
}

/** API tendue à un plugin, taillée par ses permissions. */
function apiPour(plugin: LoadedPlugin): Record<string, unknown> {
  const permises = new Set(plugin.allowedCommands);
  return {
    version: plugin.version,
    pluginId: plugin.id,

    /** Déclare un rendu à un point d'accroche. */
    onMount(point: MountPoint, render: Renderer): void {
      if (!plugin.mount.includes(point)) {
        throw new Error(
          `le plugin ${plugin.id} n'a pas déclaré le point d'accroche « ${point} »`,
        );
      }
      const liste = montages.get(point) ?? [];
      liste.push({ pluginId: plugin.id, pluginName: plugin.name, render });
      montages.set(point, liste);
    },

    /** Invoque une commande du harness, si les permissions l'ouvrent. */
    call<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
      if (!permises.has(command)) {
        return Promise.reject(
          new Error(
            `« ${command} » n'est pas ouverte au plugin ${plugin.id} ` +
              `(permissions : ${plugin.permissions.join(", ") || "aucune"})`,
          ),
        );
      }
      return call<T>(command, args);
    },

    /** Écoute un événement du harness. */
    subscribe<T>(event: string, cb: (payload: T) => void): Promise<() => void> {
      return subscribe<T>(event, cb);
    },
  };
}

/**
 * Recharge les plugins actifs.
 *
 * Les montages précédents sont oubliés : recharger doit repartir d'un état
 * propre, sans laisser derrière les accroches d'un plugin désactivé.
 */
export async function recharger(): Promise<{ charges: number; erreurs: string[] }> {
  montages.clear();
  erreursChargement = [];

  let plugins: LoadedPlugin[] = [];
  try {
    plugins = await ipc.pluginsLoad();
  } catch (e) {
    erreursChargement.push(e instanceof Error ? e.message : String(e));
    notifier();
    return { charges: 0, erreurs: erreursChargement };
  }

  let charges = 0;
  for (const plugin of plugins) {
    try {
      // `new Function` isole la portée lexicale, pas les capacités : voir
      // l'avertissement en tête de module.
      const fabrique = new Function("obsia", `"use strict";\n${plugin.source}`);
      fabrique(apiPour(plugin));
      charges += 1;
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      erreursChargement.push(`${plugin.name} : ${message}`);
    }
  }
  notifier();
  return { charges, erreurs: erreursChargement };
}
