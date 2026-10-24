#!/usr/bin/env python3
"""Secret scan — issue #25 ("Secret scan in CI (PATs, keys — the
.gitignore policy executable)").

Three executable checks (run from the repo root; pure stdlib, fast):

1. CONTENT SCAN — every git-tracked file is scanned for high-signal
   credential patterns (GitHub PATs classic + fine-grained, AWS keys,
   Slack tokens, Stripe keys, Google API keys, GitLab PATs, npm tokens,
   private key blocks, ...). An explicit allowlist (path, pattern,
   reason) covers self-referential hits (e.g. a docs line that quotes a
   pattern shape to warn against it) — every allowlist entry must carry
   a reason and is itself reviewed.

2. GITIGNORE POLICY — the repo's .gitignore must contain the mandatory
   credential entries (*.pem, *.key, .env, secrets/, *gh_token*, ...).
   A dropped policy line fails the scan instead of silently weakening
   it.

3. TRACKED-DESPITE-POLICY — `git ls-files --ignored --cached
   --exclude-standard` must be empty: nothing that the ignore policy
   says should never be committed may be tracked. (Un-ignore exceptions
   such as the provenance-checked dummy .onnx fixtures are expressed in
   .gitignore itself and therefore do not appear here.)

Exit code 0 = clean; 1 = findings (printed, one per line).
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# ── 1. Credential patterns (high-signal only — no generic
#    "password=" noise that buries real findings) ────────────────────────────

SECRET_PATTERNS: list[tuple[str, re.Pattern[str]]] = [
    # GitHub — fine-grained PAT (this repo's own token class; the reason
    # this scan exists) and classic tokens.
    ("github fine-grained PAT", re.compile(r"github_pat_[A-Za-z0-9_]{22,}")),
    ("github classic PAT", re.compile(r"\bghp_[A-Za-z0-9]{30,}\b")),
    ("github OAuth token", re.compile(r"\bgho_[A-Za-z0-9]{30,}\b")),
    ("github app/server token", re.compile(r"\bgh[sr]_[A-Za-z0-9]{30,}\b")),
    # Cloud / SaaS.
    ("AWS access key id", re.compile(r"\bAKIA[0-9A-Z]{16}\b")),
    ("AWS secret access key", re.compile(r"(?i)aws.{0,30}?['\"][0-9a-zA-Z/+]{40}['\"]")),
    ("Google API key", re.compile(r"\bAIza[0-9A-Za-z_-]{35}\b")),
    ("Slack token", re.compile(r"\bxox[baprs]-[0-9A-Za-z-]{10,}\b")),
    ("Stripe secret key", re.compile(r"\b[sr]k_live_[0-9a-zA-Z]{20,}\b")),
    ("GitLab PAT", re.compile(r"\bglpat-[0-9A-Za-z_-]{20,}\b")),
    ("npm access token", re.compile(r"\bnpm_[0-9A-Za-z]{36}\b")),
    ("PyPI token", re.compile(r"\bpypi-AgEIcHlwaS5vcmc[A-Za-z0-9_-]{20,}\b")),
    ("OpenAI-style key", re.compile(r"\bsk-[A-Za-z0-9_-]{40,}\b")),
    # Key material blocks.
    ("private key block", re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY( BLOCK)?-----")),
    ("PuTTY user key", re.compile(r"PuTTY-User-Key-File-\d")),
    ("SSH private key marker", re.compile(r"-----BEGIN OPENSSH PRIVATE KEY-----")),
]

# Self-referential / documented mentions. Every entry needs a reason.
ALLOWLIST: list[tuple[str, str, str]] = [
    # (path, pattern-name, reason)
    ("scripts/secret_scan.py", "github fine-grained PAT",
     "the scanner's own pattern source — the literal class characters "
     "never form a real token"),
    ("scripts/secret_scan.py", "private key block",
     "the scanner's own pattern source (and the OPENSSH sample string) "
     "— shape documentation, never key material"),
    ("scripts/secret_scan.py", "SSH private key marker",
     "the scanner's own pattern source — shape documentation, never key material"),
    ("scripts/validate_docs.py", "github fine-grained PAT",
     "the phase-0 docs validator carries the same pattern source"),
    ("docs/agent/SECURITY_NOTES.md", "*",
     "if it ever documents a pattern shape, that is the point of the doc"),
]

# ── 2. Mandatory .gitignore policy lines (the policy made executable) ──────

REQUIRED_IGNORES: list[str] = [
    "*.pem",
    "*.key",
    "*.p12",
    "*.pfx",
    ".env",
    ".env.*",
    "secrets/",
    "*gh_token*",
    "*github_pat*",
    "app-signing-key*",
    "*.minisign",
]

# Scan skip list: vendored lockfiles are huge and pattern-noisy in
# exactly the ways that produce false positives (base64-ish hashes).
# Real secrets never live in lockfiles by construction; the tracked-
# despite-policy check still covers them.
SKIP_SUFFIXES = (".lock",)


def git_tracked_files() -> list[str]:
    out = subprocess.run(
        ["git", "ls-files"], cwd=REPO, capture_output=True, text=True, check=True
    ).stdout
    return [f for f in out.splitlines() if f.strip()]


def allowed(path: str, pattern_name: str) -> bool:
    for a_path, a_pat, _reason in ALLOWLIST:
        if path == a_path and (a_pat == "*" or a_pat == pattern_name):
            return True
    return False


def scan_content(files: list[str]) -> list[str]:
    findings: list[str] = []
    for f in files:
        if f.endswith(SKIP_SUFFIXES):
            continue
        p = REPO / f
        if not p.is_file():
            continue
        try:
            text = p.read_bytes()
        except OSError:
            continue
        # Decode permissively: secrets are ASCII; non-UTF-8 files (real
        # binary assets) cannot contain these token classes anyway.
        try:
            text_s = text.decode("utf-8", errors="ignore")
        except AttributeError:
            text_s = text
        for name, pat in SECRET_PATTERNS:
            if pat.search(text_s):
                if allowed(f, name):
                    continue
                findings.append(f"SECRET PATTERN [{name}] in {f}")
    return findings


def check_gitignore_policy() -> list[str]:
    gi = REPO / ".gitignore"
    if not gi.is_file():
        return [".gitignore missing — the credential policy is gone"]
    lines = {ln.strip() for ln in gi.read_text().splitlines()}
    missing = [req for req in REQUIRED_IGNORES if req not in lines]
    return [f".gitignore policy line missing: {req}" for req in missing]


def check_tracked_despite_policy() -> list[str]:
    out = subprocess.run(
        ["git", "ls-files", "--ignored", "--cached", "--exclude-standard"],
        cwd=REPO,
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    offenders = [ln for ln in out.splitlines() if ln.strip()]
    return [f"TRACKED despite ignore policy: {f}" for f in offenders]


def main() -> int:
    files = git_tracked_files()
    findings = scan_content(files)
    findings += check_gitignore_policy()
    findings += check_tracked_despite_policy()
    if findings:
        print("secret scan: FAIL")
        for f in findings:
            print(f"  - {f}")
        return 1
    print(
        f"secret scan: PASS ({len(files)} tracked files scanned, "
        f"{len(SECRET_PATTERNS)} patterns, gitignore policy enforced)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
