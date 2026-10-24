#!/usr/bin/env node
// MELOGRAPHY — WCAG contrast audit (issue #15, UX_SPEC §10).
//
// Parses the design tokens in apps/melography-app/ui/src/app.css (dark
// default + light + contrast themes) and COMPUTES — not assumes — the
// WCAG 2.1 contrast ratio for every token pair the UI actually paints.
//
// Gates (WCAG AA):
//   * normal text (< 18.66px bold / < 24px): ratio ≥ 4.5
//   * UI / large-text accents (focus ring, ≥ 18.66px bold): ≥ 3.0
//   * disabled text is EXEMPT per WCAG (documented, not asserted)
//
// Usage: node scripts/ui_contrast_audit.mjs   (from the repo root)
// Prints the full per-theme table; exit 1 on any failure.

import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const css = readFileSync(join(root, "apps", "melography-app", "ui", "src", "app.css"), "utf8");

// ── Parse themes ──────────────────────────────────────────────────────────
function parseBlock(headerRe, label) {
  const m = css.match(headerRe);
  if (!m) throw new Error(`theme block ${label} not found`);
  const body = m[1];
  const tokens = {};
  for (const t of body.matchAll(/--([a-z0-9-]+):\s*(#[0-9a-fA-F]{6})/g)) {
    tokens[`--${t[1]}`] = t[2].toLowerCase();
  }
  return tokens;
}

const themes = {
  dark: parseBlock(/:root\s*\{([\s\S]*?)\n\}/, "dark"),
  light: parseBlock(/:root\[data-theme="light"\]\s*\{([\s\S]*?)\n\}/, "light"),
  contrast: parseBlock(/:root\[data-theme="contrast"\]\s*\{([\s\S]*?)\n\}/, "contrast"),
};

// ── WCAG 2.1 relative luminance + ratio ───────────────────────────────────
function channel(c) {
  const s = c / 255;
  return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
}
function luminance(hex) {
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}
function ratio(fg, bg) {
  const l1 = luminance(fg);
  const l2 = luminance(bg);
  const [hi, lo] = l1 >= l2 ? [l1, l2] : [l2, l1];
  return (hi + 0.05) / (lo + 0.05);
}

// ── The pairs the UI paints (each row names where it appears) ─────────────
// threshold 4.5 = normal text; 3.0 = large text / UI components.
const PAIRS = [
  ["fg-primary", "bg-app", 4.5, "body text, toolbar, statusbar"],
  ["fg-primary", "bg-panel", 4.5, "panel text"],
  ["fg-primary", "bg-elevated", 4.5, "dialog text, buttons"],
  ["fg-primary", "bg-hover", 4.5, "hovered button text"],
  ["fg-secondary", "bg-panel", 4.5, "slider labels, secondary panel text"],
  ["fg-secondary", "bg-elevated", 4.5, "badge text, dialog secondary"],
  ["fg-tertiary", "bg-panel", 4.5, "hints, profile notes (11-12px)"],
  ["fg-tertiary", "bg-elevated", 4.5, "tertiary text on dialogs/buttons"],
  ["fg-secondary", "bg-app", 4.5, "statusbar muted text"],
  ["ok", "bg-panel", 4.5, "done status chips (S6)"],
  ["info", "bg-panel", 4.5, "info badges"],
  ["warn", "bg-panel", 4.5, "amber badges (provenance, warnings)"],
  ["danger", "bg-panel", 4.5, "error chips/text"],
  ["accent-400", "bg-app", 3.0, "focus ring (UI component)"],
  ["accent-400", "bg-panel", 3.0, "active tab accent, filmstrip active ring"],
  ["accent-500", "bg-app", 3.0, "primary button edge (non-text)"],
];

// primary button text is a fixed dark ink on the accent fill (app.css)
const PRIMARY_TEXT = "#141414";

let failures = 0;
const width = 22;
console.log("WCAG AA contrast audit (computed from app.css tokens)\n");
for (const [name, tokens] of Object.entries(themes)) {
  console.log(`theme: ${name}`);
  console.log(`  ${"pair".padEnd(width)} ${"ratio".padStart(6)}  gate  where`);
  for (const [fg, bg, gate, where] of PAIRS) {
    const fgv = tokens[`--${fg}`];
    const bgv = tokens[`--${bg}`];
    if (!fgv || !bgv) {
      console.error(`  FAIL ${fg} on ${bg}: token missing in theme ${name}`);
      failures++;
      continue;
    }
    const r = ratio(fgv, bgv);
    const pass = r >= gate;
    if (!pass) failures++;
    console.log(
      `  ${` ${fg} on ${bg}`.padEnd(width)} ${r.toFixed(2).padStart(6)}  ${pass ? "≥" : "<"}${gate}  ${where}${pass ? "" : "  ← FAIL"}`,
    );
  }
  // primary button: dark ink on accent fill (normal text, 4.5)
  for (const accent of ["accent-500", "accent-400"]) {
    const r = ratio(PRIMARY_TEXT, tokens[`--${accent}`]);
    const pass = r >= 4.5;
    if (!pass) failures++;
    console.log(
      `  ${`primary text on ${accent}`.padEnd(width)} ${r.toFixed(2).padStart(6)}  ${pass ? "≥" : "<"}4.5  primary button label${pass ? "" : "  ← FAIL"}`,
    );
  }
  console.log("");
}
console.log(
  "note: --fg-disabled is WCAG-exempt (disabled controls); the viewport canvas is image content, not UI text.",
);
console.log(failures === 0 ? "contrast audit: PASS" : `contrast audit: ${failures} failure(s)`);
process.exit(failures === 0 ? 0 : 1);
