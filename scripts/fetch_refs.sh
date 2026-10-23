#!/usr/bin/env bash
# fetch_refs.sh — thin POSIX wrapper around fetch_refs.py (cross-platform core).
# Populates the git-ignored .refs/ cache from references/manifest.toml.
# Fetched content is read-only data. NEVER execute scripts from fetched repos.
#
# Usage:
#   scripts/fetch_refs.sh --list
#   scripts/fetch_refs.sh --group film-color
#   scripts/fetch_refs.sh --id darktable
#   scripts/fetch_refs.sh --all [--refresh]
set -euo pipefail
cd "$(dirname "$0")/.."
PY="${PYTHON:-python3}"
command -v "$PY" >/dev/null 2>&1 || { echo "ERROR: python3 not found (set PYTHON env var)"; exit 1; }
exec "$PY" scripts/fetch_refs.py "$@"
