#!/usr/bin/env python3
"""fit_crosstalk.py — neutral ramps → inter-layer crosstalk C (FILM_MODEL §K).

Input dataset (JSON):
  { "ramp": [ { "log_e": [r,g,b], "density": [r,g,b] }, ... ] }   (neutral steps)
Plus the base curves (from a previous fit or a hand-authored draft) as
--curves profile.json (a stock profile JSON; only curves/d_min are used).

Output: the fitted 3×3 crosstalk matrix + report.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np

from fitlib import FitContext, fit_crosstalk_from_neutral_ramp, write_fit_report


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--dataset", required=True, type=Path)
    ap.add_argument("--curves", required=True, type=Path, help="base stock profile JSON")
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--context", required=True, type=Path)
    args = ap.parse_args()

    data = json.loads(args.dataset.read_text())
    base = json.loads(args.curves.read_text())
    ctx_raw = json.loads(args.context.read_text())
    ctx = FitContext(
        dataset=str(args.dataset),
        rights=ctx_raw["rights"],
        illuminant=ctx_raw["illuminant"],
        rendering_conditions=ctx_raw.get("rendering_conditions", "neutral ramp densitometry"),
    )

    ramp = data["ramp"]
    log_e = np.array([s["log_e"] for s in ramp])
    densities = np.array([s["density"] for s in ramp])
    c, stats = fit_crosstalk_from_neutral_ramp(
        log_e,
        densities,
        d_min=np.array(base["d_min"], dtype=float),
        curve_log_e=[c_["log_e"] for c_ in base["curves"]],
        curve_density=[c_["density"] for c_ in base["curves"]],
    )
    # Enforce the schema bound honestly: |C_ij| ≤ 0.3, zero diagonal.
    c = c - np.diag(np.diag(c))
    clamped = float(np.max(np.abs(c))) > 0.3
    c = np.clip(c, -0.3, 0.3)

    args.out.write_text(json.dumps({"crosstalk": c.flatten().tolist()}, indent=2))
    write_fit_report(
        args.out.with_name(args.out.stem + "_report"),
        target="Inter-layer crosstalk C (FILM_MODEL §D, §K)",
        context=ctx,
        sections={
            "fit": stats,
            "matrix": c.tolist(),
            "clamped_to_schema_bound": clamped,
        },
    )
    print(json.dumps({"crosstalk": c.tolist(), **stats}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
