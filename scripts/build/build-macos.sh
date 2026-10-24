#!/usr/bin/env bash
# MELOGRAPHY — macOS build script (issue #25 session 10; docs/BUILDING.md).
#
# Builds the editor app with Tauri on macOS 12+: .app + .dmg, native or
# universal. Checks the Xcode Command Line Tools and Rust/Node toolchains,
# optionally adds the second Rust target for a universal build.
#
# The output is an UNSIGNED local build — codesigning/notarization use
# owner-provisioned certificates in the M5 release workflow
# (docs/RELEASING.md §3); this script never signs, notarizes, or
# publishes anything. To run an unsigned .app locally: right-click →
# Open (the first launch bypasses Gatekeeper for this build only).
#
# Usage:
#   scripts/build/build-macos.sh                  # native arch, dmg+app bundles
#   scripts/build/build-macos.sh --arch universal # aarch64 + x86_64 in one .app
#   scripts/build/build-macos.sh --arch aarch64|x64
#   scripts/build/build-macos.sh --bin-only       # executable only
#   scripts/build/build-macos.sh --core           # also run the workspace test gate
#   MELO_BUNDLES="app" scripts/build/build-macos.sh        # env override
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"   # repo root
APP="$HERE/apps/melography-app"
BUNDLES="${MELO_BUNDLES:-app,dmg}"
ARCH="native" BIN_ONLY=0 DO_CORE=0

while [ $# -gt 0 ]; do
  case "$1" in
    --arch)       shift; ARCH="${1:?--arch needs aarch64|x64|universal|native}" ;;
    --arch=*)     ARCH="${1#*=}" ;;
    --bin-only)   BIN_ONLY=1 ;;
    --core)       DO_CORE=1 ;;
    --bundles)    shift; BUNDLES="${1:?--bundles needs a value}" ;;
    --bundles=*)  BUNDLES="${1#*=}" ;;
    -h|--help)    sed -n '2,17p' "$0"; exit 0 ;;
    *) echo "unknown option: $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done

case "$ARCH" in
  native|aarch64|x64|universal) ;;
  *) echo "--arch must be aarch64|x64|universal|native" >&2; exit 2 ;;
esac

log()  { printf '\n\033[1;34m== %s\033[0m\n' "$*"; }
need() { command -v "$1" >/dev/null 2>&1 || { echo "MISSING: $1 — $2" >&2; exit 1; }; }

# ── 1. Toolchain checks ─────────────────────────────────────────────────────
log "Toolchain"
[ -n "$(xcode-select -p 2>/dev/null)" ] || {
  echo "MISSING: Xcode Command Line Tools — run: xcode-select --install" >&2; exit 1; }
need cargo "install Rust via https://rustup.rs (rust-toolchain.toml pins 1.99.1)"
need node  "install Node.js >= 18 (20 LTS recommended)"
need npm   "comes with Node.js"
cargo --version; node --version

# ── 2. Optional second target for universal builds ──────────────────────────
TARGET_ARGS=()
if [ "$ARCH" = "universal" ]; then
  log "Ensuring both Rust targets"
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  TARGET_ARGS=(--target universal-apple-darwin)
elif [ "$ARCH" = "aarch64" ]; then
  rustup target add aarch64-apple-darwin
  TARGET_ARGS=(--target aarch64-apple-darwin)
elif [ "$ARCH" = "x64" ]; then
  rustup target add x86_64-apple-darwin
  TARGET_ARGS=(--target x86_64-apple-darwin)
fi

# ── 3. Optional core gate ───────────────────────────────────────────────────
if [ "$DO_CORE" -eq 1 ]; then
  log "Core workspace test gate"
  (cd "$HERE" && cargo test --workspace)
fi

# ── 4. Build ────────────────────────────────────────────────────────────────
log "Building the editor app (Tauri, arch=$ARCH)"
cd "$APP"
npm --prefix ui ci
export PATH="$PWD/ui/node_modules/.bin:$PATH"
need tauri "ui/node_modules/.bin/tauri missing after npm ci"

if [ "$BIN_ONLY" -eq 1 ]; then
  tauri build --no-bundle "${TARGET_ARGS[@]}"
  log "Artifact"
  find "$APP/target" -type f -name 'melography-app' \
    -path '*release*' -exec ls -lh {} \; -exec shasum -a 256 {} \;
else
  tauri build --bundles "$BUNDLES" "${TARGET_ARGS[@]}"
  log "Artifacts (unsigned local build — see docs/RELEASING.md for the signed M5 flow)"
  find "$APP/target" -type f \( -name '*.dmg' -o -name '*.app' -prune \) -print
  find "$APP/target" -type f -name '*.dmg' -exec shasum -a 256 {} \;
  echo "(.app bundles are directories; the checksum line covers the .dmg images)"
fi
log "Done. Unsigned first launch: right-click the .app → Open."
