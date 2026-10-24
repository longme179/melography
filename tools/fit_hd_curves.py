#!/usr/bin/env python3
"""fit_hd_curves.py — densitometric H&D sets → §D monotone control points.

Input dataset (JSON): { "samples": [ { "log_e": [r, g, b], "density": [r, g, b] }, ... ] }
Output: fitted per-layer control points + a fit report (residuals, method),
written next to --out.

Usage:
  python3 tools/fit_hd_curves.py --dataset data.json --out hd_fit.json \
      --context context.json
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np

from fitlib import FitContext, fit_hd_curve_monotone_lsq, is_monotone, write_fit_report


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--dataset", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--context", required=True, type=Path, help="dataset rights/illuminant JSON")
    ap.add_argument("--control-points", type=int, default=7)
    args = ap.parse_args()

    data = json.loads(args.dataset.read_text())
    ctx_raw = json.loads(args.context.read_text())
    ctx = FitContext(
        dataset=str(args.dataset),
        rights=ctx_raw["rights"],
        illuminant=ctx_raw["illuminant"],
        rendering_conditions=ctx_raw.get("rendering_conditions", "densitometry, status-M proxy"),
    )

    samples = data["samples"]
    layers = {}
    for c, name in enumerate("rgb"):
        log_e = np.array([s["log_e"][c] for s in samples])
        dens = np.array([s["density"][c] for s in samples])
        knots, vals, stats = fit_hd_curve_monotone_lsq(log_e, dens, args.control_points)
        assert is_monotone(knots, vals), f"layer {name}: fit must be monotone"
        layers[name] = {"log_e": knots, "density": vals, "stats": stats}

    payload = {
        "schema": "melography.fit.hd-curves",
        "layers": {k: {"log_e": v["log_e"], "density": v["density"]} for k, v in layers.items()},
    }
    args.out.write_text(json.dumps(payload, indent=2))
    write_fit_report(
        args.out.with_name(args.out.stem + "_report"),
        target="H&D characteristic curves (FILM_MODEL §D, §K)",
        context=ctx,
        sections={
            "per_layer": {k: v["stats"] for k, v in layers.items()},
            "note": (
                "Control points are response-above-fog densities; the engine "
                "adds d_min on top (spec-literal). Validate γ ∈ [0.3, 1.6] "
                "before shipping a profile."
            ),
        },
    )
    print(json.dumps({k: v["stats"] for k, v in layers.items()}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
