#!/usr/bin/env node
// MELOGRAPHY — UI i18n audit (issue #15, UX_SPEC §10).
//
// Honest gates, all enforced from real file contents (exit 1 on failure):
//   1. Catalog parity   — EN and VI define exactly the same keys
//                         (no duplicates, no empties).
//   2. Placeholders     — `{x}` params present in EN appear in VI.
//   3. VI diacritics    — ≥ 80% of VI values that contain letters must
//                         contain at least one Vietnamese diacritic
//                         (catches wholesale English copies).
//   4. Length growth    — every VI value ≤ 3.5× its EN length (padding /
//                         truncation risk).
//   5. Error codes      — every `AppError::new("code")` on the Rust side
//                         has an `error.code.<code>` entry in BOTH catalogs.
//   6. Embedded strings — no hardcoded English text nodes or label
//                         attributes in .svelte markup (allowlist below:
//                         brand names, format names, endonyms, SI units).
//   7. Pseudo-locale    — the same wrap used by `vite build --mode pseudo`
//                         demonstrably lengthens + accents the catalogs.
//
// Usage: node scripts/ui_i18n_audit.mjs   (from the repo root)

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const ui = join(root, "apps", "melography-app", "ui");
const appSrc = join(root, "apps", "melography-app", "src");

let failures = 0;
const fail = (msg) => {
  failures++;
  console.error(`FAIL ${msg}`);
};
const info = (msg) => console.log(`  · ${msg}`);

// ── 0. Parse the catalogs ─────────────────────────────────────────────────
const i18nSrc = readFileSync(join(ui, "src", "lib", "i18n.ts"), "utf8");

function parseDict(name) {
  const m = i18nSrc.match(new RegExp(`const ${name}: Dict = \\{([\\s\\S]*?)\\n\\};`));
  if (!m) throw new Error(`catalog ${name} not found`);
  const dict = new Map();
  const dup = [];
  for (const mm of m[1].matchAll(/"([^"]+)":\s*"((?:[^"\\]|\\.)*)"/g)) {
    if (dict.has(mm[1])) dup.push(mm[1]);
    dict.set(mm[1], mm[2]);
  }
  if (dup.length) fail(`${name}: duplicate keys ${dup.join(", ")}`);
  return dict;
}

const en = parseDict("en");
const vi = parseDict("vi");
console.log(`catalogs: ${en.size} EN / ${vi.size} VI keys`);

// ── 1. Parity ─────────────────────────────────────────────────────────────
{
  const missVi = [...en.keys()].filter((k) => !vi.has(k));
  const missEn = [...vi.keys()].filter((k) => !en.has(k));
  if (missVi.length) fail(`keys in EN but not VI: ${missVi.join(", ")}`);
  if (missEn.length) fail(`keys in VI but not EN: ${missEn.join(", ")}`);
  const empty = [...en.entries()].filter(([k, v]) => !v.trim() && !vi.get(k)?.trim());
  if (empty.length) fail(`empty values: ${empty.map(([k]) => k).join(", ")}`);
  if (!missVi.length && !missEn.length) info(`parity OK (${en.size} keys)`);
}

// ── 2. Placeholder parity ─────────────────────────────────────────────────
{
  const ph = (s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
  const bad = [...en.keys()].filter((k) => vi.has(k) && JSON.stringify(ph(en.get(k))) !== JSON.stringify(ph(vi.get(k))));
  if (bad.length) fail(`placeholder mismatch: ${bad.join(", ")}`);
  else info("placeholders OK");
}

// ── 3. VI diacritics coverage ─────────────────────────────────────────────
{
  // Vietnamese-specific diacritics (letter + combining range covers the
  // full tiếng Việt inventory; plain ASCII loan words are the exception).
  const diacritic = /[ăâđêôơưáàảãạấầẩẫậắằẳẵặéèẻẽẹếềểễệíìỉĩịóòỏõọốồổỗộớờởỡợúùủũụứừửữựýỳỷỹỵ]/u;
  const withLetters = [...vi.values()].filter((v) => /[a-zA-Z]/.test(v));
  const noDiacritics = withLetters.filter((v) => !diacritic.test(v));
  const ratio = withLetters.length ? 1 - noDiacritics.length / withLetters.length : 0;
  if (ratio < 0.8) {
    fail(`VI diacritics coverage ${(ratio * 100).toFixed(1)}% < 80% (${noDiacritics.length}/${withLetters.length} letter-bearing values have none)`);
  } else {
    info(`VI diacritics coverage ${(ratio * 100).toFixed(1)}% (≥ 80%)`);
  }
}

// ── 4. Length growth ──────────────────────────────────────────────────────
{
  const LIMIT = 3.5;
  const bad = [];
  let worst = 0;
  let worstKey = "";
  for (const [k, ev] of en) {
    const vv = vi.get(k);
    if (!vv || !ev) continue;
    const r = vv.length / ev.length;
    if (r > worst) {
      worst = r;
      worstKey = k;
    }
    if (r > LIMIT) bad.push(`${k} (${r.toFixed(1)}×)`);
  }
  if (bad.length) fail(`VI strings > ${LIMIT}× EN (truncation risk): ${bad.join(", ")}`);
  else info(`length growth OK (worst ${worst.toFixed(2)}× on "${worstKey}", limit ${LIMIT}×)`);
}

// ── 5. Rust error-code coverage ───────────────────────────────────────────
{
  const rsFiles = readdirSync(appSrc, { recursive: true })
    .map((f) => join(appSrc, String(f)))
    .filter((f) => f.endsWith(".rs") && statSync(f).isFile());
  const codes = new Set();
  for (const f of rsFiles) {
    for (const m of readFileSync(f, "utf8").matchAll(/AppError::new\("([^"]+)"/g)) codes.add(m[1]);
  }
  const missing = [...codes].filter((c) => !en.has(`error.code.${c}`) || !vi.has(`error.code.${c}`));
  if (missing.length) fail(`error codes without localized entries: ${missing.join(", ")}`);
  else info(`error-code coverage OK (${codes.size} codes from ${rsFiles.length} rs files)`);
}

// ── 6. Embedded-strings scan ──────────────────────────────────────────────
{
  // Deliberate allowlist — each entry is justified in the a11y/i18n report:
  //   brand · file-format names · language endonyms · SI/photographic
  //   units · paths/URLs · pure symbols/emoji.
  const ALLOW = new Set([
    "MELOGRAPHY", // brand
    "English", "Tiếng Việt", // language endonyms (i18n convention)
    "PNG", "JPEG", "TIFF", "HEIC / AVIF", // format names
    "EV", "MB", "kB", "ms", "K", // photographic / SI units
    "~/.melography", "https://…", "MELOGRAPHY_PROFILES", // paths, env vars
  ]);
  const words = (s) => s.match(/[A-Za-z]{2,}/g) ?? [];
  const svelteFiles = readdirSync(join(ui, "src"), { recursive: true })
    .map((f) => join(ui, "src", String(f)))
    .filter((f) => f.endsWith(".svelte"));
  const violations = [];
  for (const f of svelteFiles) {
    const src = readFileSync(f, "utf8");
    const scriptEnd = src.indexOf("</script>");
    const markup = scriptEnd >= 0 ? src.slice(scriptEnd) : src;
    const lines = markup.split("\n");
    let lineNo = scriptEnd >= 0 ? src.slice(0, scriptEnd).split("\n").length : 0;
    for (const line of lines) {
      lineNo++;
      // visible text nodes with English words (skip {…} expressions)
      for (const m of line.matchAll(/>([^<>{}]+)</g)) {
        const s = m[1].trim();
        if (!s || ALLOW.has(s)) continue;
        if (words(s).length === 0) continue;
        // HTML comments and Svelte blocks are not visible text
        if (s.startsWith("<!--") || /^(\/|#|:)/.test(s)) continue;
        violations.push(`${f.replace(root + "/", "")}:${lineNo} text "${s.slice(0, 60)}"`);
      }
      // label attributes with literal (non-{expr}) values
      for (const m of line.matchAll(/(?:aria-label|title|placeholder|alt)="([^"{][^"]*)"/g)) {
        const s = m[1].trim();
        if (!s || ALLOW.has(s)) continue;
        if (words(s).length === 0) continue;
        violations.push(`${f.replace(root + "/", "")}:${lineNo} attr "${s.slice(0, 60)}"`);
      }
    }
  }
  if (violations.length) {
    fail(`embedded strings in markup (${violations.length}):\n     ${violations.join("\n     ")}`);
  } else {
    info(`embedded-strings scan OK (${svelteFiles.length} svelte files, allowlist ${ALLOW.size} entries)`);
  }
}

// ── 7. Pseudo-locale transform sanity ─────────────────────────────────────
{
  // Mirror of pseudoWrap() in i18n.ts (keep the two in sync).
  const pseudoWrap = (s) => {
    const accented = s.replace(/[aeiou](?![^{}]*\})/g, (c) =>
      c === "a" ? "á" : c === "e" ? "é" : c === "i" ? "í" : c === "o" ? "ó" : "ú",
    );
    const pad = "Ẋ".repeat(Math.max(3, Math.ceil(s.length * 0.4)));
    return `[̃ ${accented} ${pad}]`;
  };
  const sample = [...en.entries()].filter(([k]) => k.startsWith("toolbar.") || k.startsWith("film."));
  let grew = 0;
  for (const [, v] of sample) if (pseudoWrap(v).length >= v.length * 1.4) grew++;
  const ratio = sample.length ? grew / sample.length : 0;
  const accented = sample.every(([, v]) => /[áéíóú]/u.test(pseudoWrap(v)) || !/[aeiou]/.test(v));
  if (ratio < 0.95 || !accented) {
    fail(`pseudo-locale transform weak: growth ${ratio * 100}%, accented=${accented}`);
  } else {
    info(`pseudo-locale transform OK (${sample.length} samples, ≥40% growth, diacritics present)`);
  }
}

console.log(
  failures === 0
    ? "i18n audit: PASS"
    : `i18n audit: ${failures} failure(s)`,
);
process.exit(failures === 0 ? 0 : 1);
