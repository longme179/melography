#!/usr/bin/env python3
"""fetch_refs.py — populate the local .refs/ cache from references/manifest.toml.

Cross-platform (Linux/macOS/Windows). Fetched content is DATA for reading only:
never execute scripts, installers, or hooks from fetched repositories.

Features:
  --list                 list manifest entries (id, category, policy, license)
  --group <category>     filter by category (skills, film-color, grain, luts, ai,
                         comparables, gpu, imaging, packaging)
  --id <id>              fetch a single entry
  --all                  fetch every entry (default: --list only when no args)
  --refresh              re-fetch even if cached
  --no-network           only use cache; never hit network (resolution errors reported)

Robustness rules (master prompt §11):
  - A manifest revision may be: full 40-hex SHA | 12-hex prefix | tag name | FETCH_AT_USE.
  - Short prefixes and FETCH_AT_USE are resolved to full SHAs via the GitHub API
    (GET /repos/{repo}/commits/{rev-or-branch}).
  - `git fetch --depth 1 origin <full-sha>` is attempted first (GitHub advertises
    arbitrary reachable SHAs); on failure we fetch the recorded branch and verify
    the pinned SHA is an ancestor; on further failure we fetch full history and
    retry the SHA; only then do we report a hard error.
  - Post-checkout we ALWAYS verify the checked-out HEAD equals the resolved SHA.
"""
from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tomllib
import urllib.error
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
MANIFEST = REPO_ROOT / "references" / "manifest.toml"
CACHE_DIR = REPO_ROOT / ".refs"
RESOLVED = CACHE_DIR / "resolved.json"
HEX40 = re.compile(r"^[0-9a-f]{40}$")
HEX12 = re.compile(r"^[0-9a-f]{12}$")

try:
    TOKEN = os.environ.get("MELOGRAPHY_GH_TOKEN", "")
except Exception:
    TOKEN = ""


def die(msg: str, code: int = 1) -> None:
    print(f"ERROR: {msg}", file=sys.stderr)
    sys.exit(code)


def load_manifest() -> dict:
    with MANIFEST.open("rb") as f:
        return tomllib.load(f)


def gh_api(url: str) -> dict:
    req = urllib.request.Request(url, headers={
        "Accept": "application/vnd.github+json",
        "User-Agent": "melography-fetch-refs",
        **({"Authorization": f"Bearer {TOKEN}"} if TOKEN else {}),
    })
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.load(r)


def resolve_full_sha(url: str, branch_or_rev: str) -> str:
    """Resolve a branch name, tag, or short SHA prefix to a full 40-hex SHA."""
    repo = url.rstrip("/").replace("https://github.com/", "")
    try:
        d = gh_api(f"https://api.github.com/repos/{repo}/commits/{branch_or_rev}")
        sha = d.get("sha", "")
        if HEX40.match(sha):
            return sha
    except (urllib.error.URLError, urllib.error.HTTPError) as e:
        raise RuntimeError(f"cannot resolve {repo}@{branch_or_rev}: {e}") from e
    raise RuntimeError(f"unresolved revision {repo}@{branch_or_rev}")


def run_git(args: list[str], cwd: Path) -> subprocess.CompletedProcess:
    env = dict(os.environ, GIT_TERMINAL_PROMPT="0")
    return subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True, env=env)


def fetch_entry(entry: dict, refresh: bool, no_network: bool) -> bool:
    rid, url, branch = entry["id"], entry["url"], entry.get("branch", "main")
    target = CACHE_DIR / rid
    resolved_map = json.loads(RESOLVED.read_text()) if RESOLVED.exists() else {}

    rev = entry.get("revision", "FETCH_AT_USE")
    if rev == "FETCH_AT_USE" or HEX12.match(rev) or not HEX40.match(rev):
        if no_network:
            full = resolved_map.get(rid, {}).get("sha")
            if not full:
                print(f"SKIP {rid}: needs network resolution and --no-network set")
                return False
        else:
            try:
                full = resolve_full_sha(url, rev if rev != "FETCH_AT_USE" else branch)
            except RuntimeError as e:
                print(f"FAIL {rid}: {e}")
                return False
    else:
        full = rev

    if target.exists() and not refresh:
        head = run_git(["rev-parse", "HEAD"], target)
        if head.returncode == 0 and head.stdout.strip() == full:
            print(f"OK   {rid} (cached) @ {full[:12]}")
            return True

    if target.exists():
        shutil.rmtree(target)
    target.mkdir(parents=True)
    g = run_git(["init", "-q"], target)
    if g.returncode != 0:
        print(f"FAIL {rid}: git init: {g.stderr.strip()}")
        return False
    run_git(["remote", "add", "origin", url], target)

    ok = False
    # Attempt 1: shallow fetch of the exact SHA (GitHub advertises reachable SHAs).
    if run_git(["fetch", "-q", "--depth", "1", "origin", full], target).returncode == 0:
        ok = True
    else:
        # Attempt 2: fetch branch, verify pinned SHA is present.
        if (run_git(["fetch", "-q", "--depth", "50", "origin", branch], target).returncode == 0
                and run_git(["cat-file", "-e", f"{full}^{{commit}}"], target).returncode == 0):
            ok = True
        else:
            # Attempt 3: full fetch (no depth), then SHA check.
            if (run_git(["fetch", "-q", "origin"], target).returncode == 0
                    and run_git(["cat-file", "-e", f"{full}^{{commit}}"], target).returncode == 0):
                ok = True
    if not ok:
        print(f"FAIL {rid}: could not fetch pinned SHA {full} (history rewritten or unreachable?)")
        shutil.rmtree(target, ignore_errors=True)
        return False

    c = run_git(["checkout", "-q", "--detach", full], target)
    if c.returncode != 0:
        print(f"FAIL {rid}: checkout: {c.stderr.strip()}")
        return False
    head = run_git(["rev-parse", "HEAD"], target).stdout.strip()
    if head != full:
        print(f"FAIL {rid}: HEAD {head[:12]} != pinned {full[:12]}")
        return False

    resolved_map[rid] = {"sha": full, "resolved_at": __import__("time").strftime("%Y-%m-%dT%H:%M:%SZ", __import__("time").gmtime())}
    RESOLVED.write_text(json.dumps(resolved_map, indent=2))
    print(f"OK   {rid} @ {full[:12]} [{entry.get('policy', '?')}]")
    return True


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--list", action="store_true", help="list manifest entries")
    ap.add_argument("--group", help="filter by category")
    ap.add_argument("--id", help="fetch a single entry")
    ap.add_argument("--all", action="store_true", help="fetch all (filtered) entries")
    ap.add_argument("--refresh", action="store_true", help="re-fetch even if cached")
    ap.add_argument("--no-network", action="store_true", help="cache-only mode")
    args = ap.parse_args()

    if not MANIFEST.exists():
        die(f"manifest not found: {MANIFEST}")
    data = load_manifest()
    entries = data.get("reference", [])
    if args.group:
        entries = [e for e in entries if e.get("category") == args.group]

    if args.list or not (args.all or args.id):
        for e in entries:
            print(f"{e['id']:<24} {e.get('category', '?'):<12} {e.get('policy', '?'):<30} {e.get('license', '?')[:40]}")
        print(f"\n{len(entries)} entries (use --all / --id / --group to fetch)")
        return

    CACHE_DIR.mkdir(exist_ok=True)
    targets = [e for e in entries if e["id"] == args.id] if args.id else entries
    if args.id and not targets:
        die(f"unknown id: {args.id}")
    results = [fetch_entry(e, args.refresh, args.no_network) for e in targets]
    failed = [e["id"] for e, r in zip(targets, results) if not r]
    if failed:
        die(f"failed entries: {', '.join(failed)}", 2)
    print("All requested references fetched. Reminder: fetched content is read-only data — never execute it.")


if __name__ == "__main__":
    main()
