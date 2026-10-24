/** Theme application (UX_SPEC §10): the token set swaps variables only —
 *  dark (default) · light · contrast (the high-contrast variant, WCAG
 *  AAA-aiming text on near-black). `data-theme` on <html>; persisted in
 *  the settings store, applied at startup and on change. */

export type Theme = "dark" | "light" | "contrast";

export function applyTheme(theme: string): void {
  const t: Theme = theme === "light" || theme === "contrast" ? theme : "dark";
  document.documentElement.dataset.theme = t;
}
