#!/usr/bin/env node
// MELOGRAPHY — UI blit smoke test (session 15).
//
// Regression gate for the editor's most frequent runtime error:
//
//     "Something needs attention; undefined is not an object
//      (evaluating 'p.width=v.width')"
//
// Root cause (verified against the minified production bundle): the
// minified `p.width=v.width` was Viewport.blit()'s
// `originalCanvas.width = original.width` — the compare canvas is bound
// by `bind:this` only while its `{#if}` block is mounted, but blit()
// touched it whenever an `original` frame existed, so EVERY render pass
// with compare mode off threw a TypeError into the error banner.
//
// The fix routes every canvas pixel write through `ui/src/lib/blit.ts`,
// where an unbound canvas / missing frame / degenerate dims / truncated
// buffer are typed no-ops. This gate:
//
//   1. Behavior — compiles the REAL `src/lib/blit.ts` with the
//      workspace's own esbuild (the same compiler vite uses — no new
//      dev dependencies) and runs it against stub DOM elements:
//        - unbound canvas (the crash above) → no-op, no throw
//        - missing frame → no-op, previous pixels preserved
//        - truncated buffer / degenerate dims → no-op, no throw
//        - null 2d context → no-op, canvas untouched (ctx acquired
//          before any resize)
//        - well-formed frame → resize + one putImageData, exact bytes
//   2. Single blit path — no file under ui/src (outside lib/blit.ts)
//      may assign `canvas.width`/`canvas.height`/`originalCanvas.…`
//      directly again; comments are stripped, so this is a code-only
//      tripwire for reintroducing the crash class at a call site.
//
// Usage: node scripts/ui_blit_smoke.mjs   (from the repo root; requires
// apps/melography-app/ui/node_modules — the CI app-build job installs
// it with `npm ci` before this gate runs)

import { readFileSync, readdirSync, statSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname, extname } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const ui = join(root, "apps", "melography-app", "ui");

let failures = 0;
const fail = (msg) => {
  failures++;
  console.error(`FAIL ${msg}`);
};
const info = (msg) => console.log(`  · ${msg}`);
const check = (cond, msg) => {
  if (cond) info(`${msg}`);
  else fail(msg);
};

// ── 0. Compile the real module with the workspace esbuild ────────────
let putRgba, blitFrame, tmpDir;
try {
  const esbuildMod = await import(
    pathToFileURL(join(ui, "node_modules", "esbuild", "lib", "main.js")).href
  );
  const esbuild = esbuildMod.default ?? esbuildMod;
  const src = readFileSync(join(ui, "src", "lib", "blit.ts"), "utf8");
  const { code } = esbuild.transformSync(src, { loader: "ts", format: "esm" });
  tmpDir = mkdtempSync(join(tmpdir(), "melography-blit-"));
  const modPath = join(tmpDir, "blit.mjs");
  writeFileSync(modPath, code);
  // Node has no ImageData global — stub the constructor (we are testing
  // OUR guards, not the browser's pixel container).
  globalThis.ImageData = class {
    constructor(data, width, height) {
      this.data = data;
      this.width = width;
      this.height = height;
    }
  };
  ({ putRgba, blitFrame } = await import(pathToFileURL(modPath).href));
} catch (e) {
  console.error(`FAIL could not compile/import ui/src/lib/blit.ts: ${e?.message ?? e}`);
  process.exit(1);
}

// Stub canvas: sentinel dims 7×5 — a no-op must leave them untouched,
// a successful blit must overwrite them.
function stubCanvas({ noContext = false } = {}) {
  const puts = [];
  return {
    width: 7,
    height: 5,
    putImageDataCalls: puts,
    getContext(type) {
      if (noContext) return null;
      return {
        putImageData(image, dx, dy) {
          puts.push({ image, dx, dy });
        },
      };
    },
  };
}

const W = 4;
const H = 3;
const rgba48 = new Uint8ClampedArray(W * H * 4);
for (let i = 0; i < rgba48.length; i++) rgba48[i] = (i * 7) & 0xff;
const frame = { width: W, height: H, renderMs: 1.25, clamped: 0, rgba: rgba48 };

console.log("1 · Behavior — the real lib/blit.ts against stub DOM elements");

// THE regression: unbound compare canvas (compare off / not mounted) —
// this exact call used to throw `undefined is not an object`.
check(blitFrame(undefined, frame) === false, "blitFrame(undefined, frame) → false (no throw)");
check(blitFrame(null, frame) === false, "blitFrame(null, frame) → false (no throw)");

// Missing frame: no-op, previous pixels preserved.
{
  const c = stubCanvas();
  check(blitFrame(c, undefined) === false, "blitFrame(canvas, undefined) → false");
  check(blitFrame(c, null) === false, "blitFrame(canvas, null) → false");
  check(c.putImageDataCalls.length === 0, "missing frame draws nothing");
  check(c.width === 7 && c.height === 5, "missing frame keeps previous dims");
}

// Well-formed frame: resize + one putImageData with the exact payload.
{
  const c = stubCanvas();
  check(blitFrame(c, frame) === true, "blitFrame(canvas, frame) → true");
  check(c.width === W && c.height === H, `canvas resized to ${W}×${H}`);
  check(c.putImageDataCalls.length === 1, "exactly one putImageData");
  const put = c.putImageDataCalls[0];
  check(put && put.dx === 0 && put.dy === 0, "putImageData at (0, 0)");
  check(
    put && put.image.data === rgba48 && put.image.width === W && put.image.height === H,
    "putImageData carries the frame's exact bytes and dims",
  );
}

// putRgba directly (the Filmstrip glyph path).
{
  const c = stubCanvas();
  check(putRgba(c, rgba48, W, H) === true, "putRgba(canvas, rgba, w, h) → true");
  check(c.putImageDataCalls.length === 1, "putRgba draws once");
}

// Truncated buffer: length ≠ w·h·4 → no-op, never half-draw.
{
  const c = stubCanvas();
  const shortRgba = new Uint8ClampedArray(10);
  check(putRgba(c, shortRgba, W, H) === false, "truncated rgba → false");
  check(c.width === 7 && c.height === 5, "truncated rgba keeps previous dims");
  check(c.putImageDataCalls.length === 0, "truncated rgba draws nothing");
}

// Degenerate dimensions.
{
  const c = stubCanvas();
  check(putRgba(c, rgba48, 0, H) === false, "width 0 → false");
  check(putRgba(c, rgba48, -2, H) === false, "negative width → false");
  check(putRgba(c, rgba48, Number.NaN, H) === false, "NaN width → false");
  check(putRgba(c, rgba48, 2.5, H) === false, "fractional width → false");
  check(putRgba(c, rgba48, W, Number.POSITIVE_INFINITY) === false, "infinite height → false");
  check(c.width === 7 && c.height === 5, "degenerate dims keep previous dims");
  check(c.putImageDataCalls.length === 0, "degenerate dims draw nothing");
}

// Null 2d context (acquired before any resize): canvas untouched.
{
  const c = stubCanvas({ noContext: true });
  check(blitFrame(c, frame) === false, "null 2d context → false (no throw)");
  check(c.width === 7 && c.height === 5, "null context keeps previous dims");
  check(c.putImageDataCalls.length === 0, "null context draws nothing");
}

// ── 2. Single blit path — no direct canvas resize outside lib/blit.ts ─
console.log("2 · Single blit path — ui/src code scan (comments stripped)");

const CODE_EXTS = new Set([".svelte", ".ts", ".js"]);
const BLIT = join(ui, "src", "lib", "blit.ts");
const offenders = [];

function walk(dir) {
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    const st = statSync(p);
    if (st.isDirectory()) walk(p);
    else if (CODE_EXTS.has(extname(p)) && p !== BLIT) scanFile(p);
  }
}

function scanFile(p) {
  let src = readFileSync(p, "utf8");
  src = src.replace(/\/\*[\s\S]*?\*\//g, " "); // block comments
  src = src.replace(/^\s*\/\/.*$/gm, " "); // line comments
  src = src.replace(/<!--[\s\S]*?-->/g, " "); // svelte/html comments
  const re = /\b(?:originalCanvas|canvas)\s*\.\s*(?:width|height)\s*=/g;
  let m;
  while ((m = re.exec(src))) offenders.push(`${p}: \`${m[0]}\``);
}

walk(join(ui, "src"));
if (offenders.length === 0) {
  info("no direct `canvas.(width|height) =` assignments outside lib/blit.ts");
} else {
  for (const o of offenders) fail(`direct canvas resize outside the single blit path — ${o}`);
}

rmSync(tmpDir, { recursive: true, force: true });

if (failures > 0) {
  console.error(`\n${failures} failure(s)`);
  process.exit(1);
}
console.log("\nPASS — blit guards hold and the single blit path is intact");
