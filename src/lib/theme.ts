/**
 * Application du thème.
 *
 * Le choix vit dans la configuration du backend, pas dans le stockage du
 * navigateur : l'intendant doit pouvoir le changer par le chat, et le réglage
 * doit survivre à un vidage du cache de la fenêtre.
 *
 * Le défaut est **sombre**, indépendamment du système. « system » est un choix
 * explicite, pas le comportement par défaut : l'application s'utilise sur de
 * longues sessions et rien ne justifie qu'elle bascule en clair parce que le
 * système l'a décidé à midi.
 */
import type { Theme } from "../types/ipc";

/** Attribut lu par les feuilles de style. */
const ATTRIBUT = "data-theme";

let ecouteur: ((e: MediaQueryListEvent) => void) | null = null;
let media: MediaQueryList | null = null;

function poser(effectif: "dark" | "light"): void {
  document.documentElement.setAttribute(ATTRIBUT, effectif);
}

/**
 * Applique un choix de thème et, pour « system », suit les changements du
 * système tant qu'un autre choix n'est pas fait.
 */
export function appliquerTheme(choix: Theme): void {
  // Un suivi précédent est retiré avant d'en poser un autre, sans quoi passer
  // de « system » à « dark » laisserait le système reprendre la main au
  // prochain basculement.
  if (media && ecouteur) {
    media.removeEventListener("change", ecouteur);
    media = null;
    ecouteur = null;
  }

  if (choix !== "system") {
    poser(choix);
    return;
  }

  media = window.matchMedia("(prefers-color-scheme: light)");
  poser(media.matches ? "light" : "dark");
  ecouteur = (e) => poser(e.matches ? "light" : "dark");
  media.addEventListener("change", ecouteur);
}
