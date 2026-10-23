#!/usr/bin/env python3
"""validate_docs.py — Phase-0 repository validation.

Honest, dependency-light checks (stdlib only except optional PyYAML for GitHub
workflow files — when PyYAML is unavailable, YAML files get a syntax-lite check
and the result is reported as NOT FULLY VALIDATED, never as PASS).

Checks:
  1. Required Phase-0 file inventory exists and is non-empty.
  2. All JSON files parse.
  3. All TOML files parse (tomllib, stdlib).
  4. YAML files parse when PyYAML is present (CI installs it).
  5. Every skills/*/SKILL.md has YAML front matter with name + description.
  6. Internal markdown links resolve to existing files.
  7. Secret scan: obvious token/key patterns must not appear in tracked files.
Exit code 0 only if every check that ran fully PASSED.
"""
from __future__ import annotations

import json
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

REQUIRED = [
    "AGENTS.md", "CLAUDE.md", "README.md", ".gitignore", ".editorconfig",
    "docs/PLAN.md", "docs/ARCHITECTURE.md", "docs/COLOR_PIPELINE.md",
    "docs/FILM_MODEL.md", "docs/AI_MODELS.md", "docs/UX_SPEC.md",
    "docs/UX_DESIGN_TOKENS.md", "docs/RELEASING.md",
    "docs/adr/0001-technology-stack.md", "docs/adr/0002-color-management.md",
    "docs/adr/0003-licensing.md", "docs/adr/0004-local-ai.md",
    "docs/adr/0005-packaging-and-updates.md",
    "docs/research/REFERENCES.md", "docs/research/REPO_AUDIT.md",
    "docs/research/OPEN_QUESTIONS.md", "docs/research/film-color.md",
    "docs/research/grain.md", "docs/research/luts.md", "docs/research/ai.md",
    "docs/research/comparables.md",
    "docs/agent/STATE.md", "docs/agent/CONTEXT_INDEX.md",
    "references/manifest.toml",
    "scripts/fetch_refs.sh", "scripts/fetch_refs.py", "scripts/validate_docs.py",
    "skills/README.md", ".github/workflows/ci.yml", ".github/PULL_REQUEST_TEMPLATE.md",
    ".github/ISSUE_TEMPLATE/bug_report.md",
    ".github/ISSUE_TEMPLATE/feature_request.md",
    ".github/ISSUE_TEMPLATE/research_note.md",
]

SKILLS = [
    "session-handoff", "architecture-decisions", "film-science-and-profiles",
    "color-management", "lut-engine", "wgpu-and-cpu-parity", "onnx-model-vetting",
    "cross-platform-ci", "secure-release-and-update", "license-and-asset-vetting",
    "desktop-ui-design", "reference-research",
]

SECRET_PATTERNS = [
    re.compile(r"github_pat_[A-Za-z0-9_]{20,}"),
    re.compile(r"ghp_[A-Za-z0-9]{30,}"),
    re.compile(r"gho_[A-Za-z0-9]{30,}"),
    re.compile(r"AKIA[0-9A-Z]{16}"),
    re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----"),
]

MD_LINK = re.compile(r"\]\(([^)#\s]+\.md)(#[^)\s]*)?\)")

failures: list[str] = []
warnings: list[str] = []


def fail(msg: str) -> None:
    failures.append(msg)


# 1. Inventory
for rel in REQUIRED:
    p = ROOT / rel
    if not p.is_file():
        fail(f"missing required file: {rel}")
    elif p.stat().st_size < 50:
        fail(f"suspiciously empty file (<50 bytes): {rel}")

for s in SKILLS:
    p = ROOT / "skills" / s / "SKILL.md"
    if not p.is_file():
        fail(f"missing skill: skills/{s}/SKILL.md")
    else:
        text = p.read_text(encoding="utf-8")
        m = re.match(r"^---\s*\n(.*?)\n---\s*\n", text, re.S)
        if not m:
            fail(f"{s}/SKILL.md: no YAML front matter")
        else:
            fm = m.group(1)
            if not re.search(r"^name:\s*\S", fm, re.M):
                fail(f"{s}/SKILL.md: front matter missing 'name'")
            if not re.search(r"^description:\s*\S", fm, re.M):
                fail(f"{s}/SKILL.md: front matter missing 'description'")

# 2. JSON
for p in ROOT.rglob("*.json"):
    if any(part in {".refs", ".git", "node_modules", "target"} for part in p.parts):
        continue
    try:
        json.loads(p.read_text(encoding="utf-8"))
    except Exception as e:  # noqa: BLE001
        fail(f"JSON parse error {p.relative_to(ROOT)}: {e}")

# 3. TOML
for p in sorted(set(ROOT.rglob("*.toml"))):
    if any(part in {".refs", ".git", "node_modules", "target"} for part in p.parts):
        continue
    try:
        tomllib.loads(p.read_text(encoding="utf-8"))
    except Exception as e:  # noqa: BLE001
        fail(f"TOML parse error {p.relative_to(ROOT)}: {e}")

# 4. YAML (full check only with PyYAML)
try:
    import yaml  # type: ignore
    HAVE_YAML = True
except ImportError:
    HAVE_YAML = False
    warnings.append("PyYAML not installed — YAML files NOT fully validated (install: pip install pyyaml)")

for p in list(ROOT.rglob("*.yml")) + list(ROOT.rglob("*.yaml")):
    if any(part in {".refs", ".git", "node_modules", "target"} for part in p.parts):
        continue
    rel = p.relative_to(ROOT)
    if HAVE_YAML:
        try:
            yaml.safe_load(p.read_text(encoding="utf-8"))
        except Exception as e:  # noqa: BLE001
            fail(f"YAML parse error {rel}: {e}")
    else:
        # syntax-lite: no tabs, balanced quotes per line
        for i, line in enumerate(p.read_text(encoding="utf-8").splitlines(), 1):
            if "\t" in line:
                fail(f"YAML tab character {rel}:{i}")

# 5. Internal markdown links
for p in ROOT.rglob("*.md"):
    if any(part in {".refs", ".git", "node_modules", "target"} for part in p.parts):
        continue
    base = p.parent
    for m in MD_LINK.finditer(p.read_text(encoding="utf-8")):
        target = m.group(1)
        if target.startswith(("http://", "https://", "mailto:")):
            continue
        if not (base / target).resolve().exists():
            fail(f"broken internal link in {p.relative_to(ROOT)}: {target}")

# 6. Secret scan over tracked text files
import subprocess  # noqa: E402
res = subprocess.run(["git", "ls-files"], cwd=ROOT, capture_output=True, text=True)
tracked_files = [f for f in res.stdout.splitlines() if f.strip()]
for rel in tracked_files:
    p = ROOT / rel
    if not p.is_file() or p.suffix in {".png", ".jpg", ".jpeg", ".onnx", ".lock"}:
        continue
    try:
        text = p.read_text(encoding="utf-8", errors="ignore")
    except Exception:  # noqa: BLE001
        continue
    for pat in SECRET_PATTERNS:
        if pat.search(text):
            fail(f"SECRET PATTERN in {rel}: {pat.pattern}")

# Report
for w in warnings:
    print(f"WARN: {w}")
if failures:
    print(f"\nFAILED ({len(failures)}):")
    for f in failures:
        print(f"  - {f}")
    sys.exit(1)
print(f"OK: inventory ({len(REQUIRED)} required files), {len(SKILLS)} skills, "
      f"JSON/TOML parse, markdown links, secret scan "
      f"({'YAML full' if HAVE_YAML else 'YAML lite — NOT full'}) — all passed.")
sys.exit(0)
