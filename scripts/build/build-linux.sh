#!/usr/bin/env bash
# MELOGRAPHY — Linux build script (issue #25 session 10; docs/BUILDING.md).
#
# Automates docs/BUILDING.md for the three supported distro families:
# Ubuntu/Debian (apt), Fedora (dnf), Arch (pacman). Detects the distro,
# offers to install the system dependencies, builds the editor app with
# Tauri, and reports the artifacts + sha256 checksums.
#
# The output is an UNSIGNED local build — signing/notarization and any
# publish step belong to the owner-gated M5 release workflow
# (docs/RELEASING.md); this script never publishes anything.
#
# Usage:
#   scripts/build/build-linux.sh                 # deps (sudo) + bundles deb,rpm,appimage
#   scripts/build/build-linux.sh --no-install    # skip the package-manager step
#   scripts/build/build-linux.sh --bin-only      # executable only (--no-bundle)
#   scripts/build/build-linux.sh --core          # also run the workspace test gate
#   scripts/build/build-linux.sh --bundles appimage
#   MELO_BUNDLES="appimage" scripts/build/build-linux.sh   # env override
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"   # repo root
APP="$HERE/apps/melography-app"
BUNDLES="${MELO_BUNDLES:-deb,rpm,appimage}"
DO_INSTALL=1 BIN_ONLY=0 DO_CORE=0

while [ $# -gt 0 ]; do
  case "$1" in
    --no-install) DO_INSTALL=0 ;;
    --bin-only)   BIN_ONLY=1 ;;
    --core)       DO_CORE=1 ;;
    --bundles)    shift; BUNDLES="${1:?--bundles needs a value}" ;;
    --bundles=*)  BUNDLES="${1#*=}" ;;
    -h|--help)    sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "unknown option: $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done

log()  { printf '\n\033[1;34m== %s\033[0m\n' "$*"; }
need() { command -v "$1" >/dev/null 2>&1 || { echo "MISSING: $1 — $2" >&2; exit 1; }; }

# ── 1. Distro detection + system packages ───────────────────────────────────
log "System dependencies"
if [ "$DO_INSTALL" -eq 1 ] && [ -f /etc/os-release ]; then
  . /etc/os-release
  case "${ID:-}${ID_LIKE:-}" in
    *ubuntu*|*debian*)
      need sudo "run with --no-install or install manually (README §Linux)"
      echo "detected: $PRETTY_NAME (apt)"
      sudo apt-get update
      sudo apt-get install -y --no-install-recommends \
        build-essential pkg-config curl file libssl-dev \
        libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev
      ;;
    *fedora*)
      need sudo "run with --no-install or install manually (README §Linux)"
      echo "detected: $PRETTY_NAME (dnf)"
      sudo dnf install -y gcc gcc-c++ glibc-devel pkgconf-pkg-config \
        openssl-devel webkit2gtk4.1-devel gtk3-devel \
        libappindicator-gtk3-devel librsvg2-devel libxdo-devel
      ;;
    *arch*)
      need sudo "run with --no-install or install manually (README §Linux)"
      echo "detected: $PRETTY_NAME (pacman)"
      sudo pacman -S --needed --noconfirm \
        base-devel pkgconf webkit2gtk-4.1 gtk3 libayatana-appindicator librsvg
      ;;
    *)
      echo "unrecognized distro '${ID:-?}' — skipping the package step."
      echo "install the equivalents from README §Linux, then re-run with --no-install."
      exit 1
      ;;
  esac
else
  echo "skipping package installation (--no-install or no /etc/os-release)"
fi

# ── 2. Toolchain checks ─────────────────────────────────────────────────────
log "Toolchain"
need cargo "install Rust via https://rustup.rs (rust-toolchain.toml pins 1.99.1)"
need node  "install Node.js >= 18 (20 LTS recommended)"
need npm   "comes with Node.js"
cargo --version; node --version; npm --version

# ── 3. Optional core gate ───────────────────────────────────────────────────
if [ "$DO_CORE" -eq 1 ]; then
  log "Core workspace test gate"
  (cd "$HERE" && cargo test --workspace)
fi

# ── 4. Build ────────────────────────────────────────────────────────────────
log "Building the editor app (Tauri)"
cd "$APP"
npm --prefix ui ci
export PATH="$PWD/ui/node_modules/.bin:$PATH"
need tauri "ui/node_modules/.bin/tauri missing after npm ci"

if [ "$BIN_ONLY" -eq 1 ]; then
  tauri build --no-bundle
  BIN="$APP/target/release/melography-app"
  log "Artifact"
  ls -lh "$BIN"
  sha256sum "$BIN"
else
  tauri build --bundles "$BUNDLES"
  log "Artifacts (unsigned local build — see docs/RELEASING.md for the signed M5 flow)"
  find "$APP/target/release/bundle" -type f \
    \( -name '*.AppImage' -o -name '*.deb' -o -name '*.rpm' \) \
    -exec ls -lh {} \; -exec sha256sum {} \;
fi
log "Done. Install with your distro's package tool, or run the AppImage directly."
echo "Note: .deb/.rpm builds do not self-update (stated in-app, RELEASING §7)."
