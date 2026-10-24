#!/usr/bin/env python3
"""fit_excitation.py — chart colors → M_exc and the output transform
(FILM_MODEL §K), with the required Levenberg–Marquardt vs CMA-ES
comparison recorded in the report.

Input dataset (JSON):
  { "patches": [ { "scene_rgb": [r,g,b] (linear, illuminant-known),
                   "measured_rgb": [r,g,b] (print-linear) }, ... ] }
Plus --base profile.json (curves/crosstalk/print fixed; M_exc and
output_matrix are fitted).

Objective: mean squared error over patches through the full forward model
(excitation → response → print). LM optimizes the 9 M_exc entries (+9
output entries with --fit-output); CMA-ES optimizes the same objective
for the comparison section.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np

from fitlib import (
    FitContext,
    MiniProfile,
    cma_es,
    full_forward,
    levenberg_marquardt,
    write_fit_report,
)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--dataset", required=True, type=Path)
    ap.add_argument("--base", required=True, type=Path, help="stock profile JSON (fixed parts)")
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--context", required=True, type=Path)
    ap.add_argument("--fit-output", action="store_true", help="also fit the output matrix")
    ap.add_argument("--exposure-scale", type=float, default=1.0)
    ap.add_argument("--generations", type=int, default=120)
    args = ap.parse_args()

    data = json.loads(args.dataset.read_text())
    base = json.loads(args.base.read_text())
    ctx_raw = json.loads(args.context.read_text())
    ctx = FitContext(
        dataset=str(args.dataset),
        rights=ctx_raw["rights"],
        illuminant=ctx_raw["illuminant"],
        rendering_conditions=ctx_raw.get("rendering_conditions", "chart patches, print-linear"),
    )

    mp = MiniProfile.from_profile_json(base)
    scene = np.array([p["scene_rgb"] for p in data["patches"]], dtype=float)
    measured = np.array([p["measured_rgb"] for p in data["patches"]], dtype=float)

    n_m = 9
    n_o = 9 if args.fit_output else 0

    def unpack(x: np.ndarray) -> MiniProfile:
        m = MiniProfile(
            m_exc=x[:n_m].reshape(3, 3).copy(),
            crosstalk=mp.crosstalk,
            curve_log_e=mp.curve_log_e,
            curve_density=mp.curve_density,
            d_min=mp.d_min,
            d_mask=mp.d_mask,
            paper_log_e=mp.paper_log_e,
            paper_density=mp.paper_density,
            paper_white_density=mp.paper_white_density,
            output_matrix=x[n_m : n_m + n_o].reshape(3, 3).copy()
            if n_o
            else mp.output_matrix,
        )
        # Keep M_exc non-negative (schema) — soft-project by abs.
        m.m_exc = np.abs(m.m_exc)
        return m

    def residual(x: np.ndarray) -> np.ndarray:
        m = unpack(x)
        pred = np.array([full_forward(m, s, args.exposure_scale) for s in scene])
        return (pred - measured).flatten()

    def cost(x: np.ndarray) -> float:
        r = residual(x)
        return float(np.mean(r * r))

    x0 = np.concatenate([mp.m_exc.flatten(), mp.output_matrix.flatten()[:n_o]])
    x_lm, info_lm = levenberg_marquardt(residual, x0, len(scene) * 3)
    x_cma, info_cma = cma_es(cost, x0, sigma0=0.08, generations=args.generations)

    # The shipped fit is the better of the two (recorded either way).
    lm_cost = info_lm["final_cost"]
    cma_cost = info_cma["final_cost"]
    best_x = x_lm if lm_cost <= cma_cost else x_cma
    m_fit = unpack(best_x)

    args.out.write_text(
        json.dumps(
            {
                "m_exc": m_fit.m_exc.flatten().tolist(),
                "output_matrix": m_fit.output_matrix.flatten().tolist(),
                "chosen_optimizer": info_lm["optimizer"] if lm_cost <= cma_cost else info_cma["optimizer"],
            },
            indent=2,
        )
    )
    write_fit_report(
        args.out.with_name(args.out.stem + "_report"),
        target="Layer excitation M_exc / output transform (FILM_MODEL §C1, §K)",
        context=ctx,
        sections={
            "levenberg_marquardt": info_lm,
            "cma_es": info_cma,
            "comparison_note": (
                "Both optimizers ran on the identical objective; the lower-cost "
                "result ships and the comparison is recorded per §K. LM wins on "
                "smooth near-quadratic objectives; CMA-ES is the robustness "
                "fallback when the objective is multimodal."
            ),
            "residual_rmse": float(np.sqrt(cost(best_x))),
        },
    )
    print(
        json.dumps(
            {
                "lm_cost": lm_cost,
                "cma_cost": cma_cost,
                "chosen": "lm" if lm_cost <= cma_cost else "cma-es",
                "m_exc": m_fit.m_exc.tolist(),
            },
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
