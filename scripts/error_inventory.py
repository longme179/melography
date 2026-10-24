#!/usr/bin/env python3
"""Error-state inventory gate (issue #25: "Complete UX_SPEC §9 ×
ARCHITECTURE §8 matrix ... + an automated check that no raw error string
leaks to the UI").

Three-way consistency, enforced on every push:

1. RUST SIDE — every `AppError::new("<code>")` literal in the app crate;
2. I18N — every `error.code.<code>` key in BOTH catalogs (EN + VI) of
   `ui/src/lib/i18n.ts`;
3. DOC — every row of the matrix table in
   `docs/quality/ERROR_STATE_INVENTORY.md`.

All three sets must be equal. (Raw-string leakage is separately enforced
by `ui_i18n_audit.mjs`'s embedded-string scan; this script keeps the
*inventory document* honest — a code without a matrix row, or a row
without a Rust producer, is a docs drift failure.)

Exit 0 = consistent; 1 = drift (printed).
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
APP_SRC = REPO / "apps" / "melography-app" / "src"
I18N = REPO / "apps" / "melography-app" / "ui" / "src" / "lib" / "i18n.ts"
INVENTORY_MD = REPO / "docs" / "quality" / "ERROR_STATE_INVENTORY.md"

# `AppError::new(` + optional whitespace/newline + the code string —
# matches both the single-line and the multi-line (rustfmt-wrapped)
# call shapes.
CODE_LIT = re.compile(r'AppError::new\(\s*"([a-z_.]+)"')
# Arrow-arm code mapping in error.rs's From impls (`E::Bomb { .. } =>
# "io.bomb"`): plain string codes, only ever produced there.
ARROW_CODE = re.compile(r'=>\s*"([a-z]+\.[a-z_]+)"')
MD_ROW = re.compile(r"^\|\s*`([a-z_.]+)`\s*\|", re.MULTILINE)


def rust_codes() -> set[str]:
    codes: set[str] = set()
    for rs in APP_SRC.rglob("*.rs"):
        codes.update(CODE_LIT.findall(rs.read_text()))
    # The From-impl mapping table lives in error.rs by design; scanning
    # every file for `=> "x.y"` would catch non-error map arms.
    err = APP_SRC / "error.rs"
    codes.update(ARROW_CODE.findall(err.read_text()))
    return codes


def i18n_codes() -> tuple[set[str], set[str]]:
    text = I18N.read_text()
    en_start = text.index("const en")
    vi_start = text.index("const vi")
    key = re.compile(r'"error\.code\.([a-z_.]+)":')
    en = set(key.findall(text[en_start:vi_start]))
    vi = set(key.findall(text[vi_start:]))
    return en, vi


def md_codes() -> set[str]:
    return set(MD_ROW.findall(INVENTORY_MD.read_text()))


def main() -> int:
    rs = rust_codes()
    en, vi = i18n_codes()
    md = md_codes()

    problems: list[str] = []
    if rs - en:
        problems.append(f"Rust codes missing EN localization: {sorted(rs - en)}")
    if rs - vi:
        problems.append(f"Rust codes missing VI localization: {sorted(rs - vi)}")
    if en - rs:
        problems.append(f"EN entries without a Rust producer: {sorted(en - rs)}")
    if vi - rs:
        problems.append(f"VI entries without a Rust producer: {sorted(vi - rs)}")
    if en != vi:
        problems.append(f"EN/VI catalog key sets differ: {sorted(en ^ vi)}")
    if rs - md:
        problems.append(f"codes without an inventory matrix row: {sorted(rs - md)}")
    if md - rs:
        problems.append(f"matrix rows without a Rust code: {sorted(md - rs)}")

    if problems:
        print("error-state inventory: FAIL")
        for p in problems:
            print(f"  - {p}")
        return 1
    print(
        f"error-state inventory: PASS ({len(rs)} codes × EN/VI × matrix rows, "
        "3-way consistent)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
