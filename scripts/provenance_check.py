#!/usr/bin/env python3
"""Provenance gate — issue #25 ("helper that verifies every committed
binary asset has a sibling provenance record (fixtures, spikes)").

Every git-tracked binary asset must carry provenance:

* a SIBLING record `<file>.provenance.json` (the onnx-dummy style:
  `foo.onnx` + `foo.onnx.provenance.json`), or
* a DIRECTORY record `provenance.json` next to the asset whose `files`
  map contains the asset's path relative to that record (the
  heif-probe style, good for fixture directories).

For every covered asset the record's `sha256` (and `size_bytes` when
present) is verified against the committed bytes — provenance that
drifted from the asset fails the gate. Records must state at least one
of `origin` / `generated_by` / `generator_script` and a `license`.

Modes:
  python3 scripts/provenance_check.py                # verify (CI mode)
  python3 scripts/provenance_check.py --write-missing \\
      --generator "<script or process>" --license "<SPDX or note>"
                                                     # scaffold records
                                                     # for uncovered assets

The scaffold writes SIBLING records (small, local, diff-reviewable);
fill in the `note` before committing. Exit 0 = every binary asset
covered and matching.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# .cube files are TEXT (ASCII float tables) — intentionally not listed.
BINARY_SUFFIXES = {
    ".png", ".jpg", ".jpeg", ".webp", ".tif", ".tiff", ".gif", ".ico",
    ".icc", ".icm", ".bin", ".exe", ".dll", ".dylib", ".so",
    ".zip", ".gz", ".7z", ".xz", ".tar",
    ".woff", ".woff2", ".ttf", ".otf",
    ".onnx", ".ort", ".safetensors",
}


def git_tracked_files() -> list[Path]:
    out = subprocess.run(
        ["git", "ls-files"], cwd=REPO, capture_output=True, text=True, check=True
    ).stdout
    return [REPO / f for f in out.splitlines() if f.strip()]


def sha256_of(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def load_json(path: Path) -> dict:
    try:
        return json.loads(path.read_text())
    except (OSError, json.JSONDecodeError):
        return {}


def find_record(asset: Path) -> tuple[dict, str] | None:
    """Return (record-dict, where) covering `asset`, or None.

    Sibling conventions (both exist in this repo's history):
      * `<full name>.provenance.json`  (fuzz corpus style)
      * `<stem>.provenance.json`       (dummy-onnx fixture style)
    Directory-level `provenance.json` records are searched up to 3
    ancestor levels, and their `files` keys may be written relative to
    any of those levels (both conventions exist: heif-probe keys are
    spike-root-relative, s3-ort keys are repo-root-relative).
    """
    candidates = [
        asset.with_name(asset.name + ".provenance.json"),
        asset.with_name(asset.stem + ".provenance.json"),
    ]
    for sibling in candidates:
        if sibling.is_file():
            rec = load_json(sibling)
            if rec:
                return rec, sibling.relative_to(REPO).as_posix()
    ancestors = [asset.parent, *list(asset.parents)[:2]]
    for parent in ancestors:
        cand = parent / "provenance.json"
        if not cand.is_file():
            continue
        rec = load_json(cand)
        files = rec.get("files", {})
        for base in [parent, *list(asset.parents)[:3], REPO]:
            try:
                rel = asset.relative_to(base).as_posix()
            except ValueError:
                continue
            if rel in files:
                entry = dict(files[rel])
                # Directory records repeat the container's context for
                # origin/license defaults when the entry omits them.
                entry.setdefault("origin", rec.get("origin", ""))
                entry.setdefault("license", rec.get("license", ""))
                return entry, cand.relative_to(REPO).as_posix()
    return None


def verify(asset: Path) -> list[str]:
    """Verify one asset; returns a list of problem strings (empty = ok)."""
    problems: list[str] = []
    found = find_record(asset)
    rel = asset.relative_to(REPO).as_posix()
    if found is None:
        return [f"NO PROVENANCE RECORD for {rel}"]
    entry, where = found
    actual_sha = sha256_of(asset)
    recorded_sha = entry.get("sha256")
    if recorded_sha != actual_sha:
        problems.append(
            f"{rel}: sha256 MISMATCH — record {where} says {recorded_sha}, "
            f"file is {actual_sha} (asset changed without updating provenance)"
        )
    recorded_size = entry.get("size_bytes")
    if recorded_size is not None and int(recorded_size) != asset.stat().st_size:
        problems.append(f"{rel}: size_bytes mismatch in {where}")
    has_source = any(entry.get(k) for k in ("origin", "generated_by", "generator_script"))
    if not has_source:
        problems.append(f"{rel}: record {where} states no origin/generator")
    if not entry.get("license"):
        problems.append(f"{rel}: record {where} states no license")
    return problems


def write_missing(generator: str, license_note: str, note: str) -> int:
    written = 0
    for asset in git_tracked_files():
        if asset.suffix.lower() not in BINARY_SUFFIXES or not asset.is_file():
            continue
        if find_record(asset) is not None:
            continue
        rec = {
            "origin": f"generated by {generator} (this project)",
            "license": license_note,
            "sha256": sha256_of(asset),
            "size_bytes": asset.stat().st_size,
            "generated": "2026-10-25",
            "note": note,
        }
        out = asset.with_name(asset.name + ".provenance.json")
        out.write_text(json.dumps(rec, indent=2) + "\n")
        print(f"wrote {out.relative_to(REPO).as_posix()}")
        written += 1
    if written == 0:
        print("nothing missing — every binary asset already has a record")
    else:
        print(f"\n{written} scaffold(s) written — review the note fields before committing")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--write-missing", action="store_true")
    ap.add_argument("--generator", default="scripts/gen_fuzz_corpus.py")
    ap.add_argument("--license", default="CC0-1.0 (project-generated data)")
    ap.add_argument("--note", default="fuzz seed corpus (deterministic recipe; dev-only Pillow)")
    args = ap.parse_args()

    if args.write_missing:
        return write_missing(args.generator, args.license, args.note)

    tracked = git_tracked_files()
    assets = [p for p in tracked if p.suffix.lower() in BINARY_SUFFIXES and p.is_file()]
    findings: list[str] = []
    for asset in assets:
        findings += verify(asset)
    if findings:
        print(f"provenance gate: FAIL ({len(assets)} binary assets checked)")
        for f in findings:
            print(f"  - {f}")
        return 1
    print(f"provenance gate: PASS ({len(assets)} binary assets, all covered + sha-verified)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
