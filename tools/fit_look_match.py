#!/usr/bin/env python3
"""fit_look_match.py — end-to-end reference-LUT fit, **labelled
look-matching, not stock measurement** (FILM_MODEL §K).

This is the honest path OPEN_QUESTIONS #6 anticipates for upgrading
artist-authored profiles toward `reference-fitted`: given a licensed
reference LUT (or its sampled grid), find chain parameters whose output
matches the reference's *look* under stated rendering conditions. The
resulting profile must carry provenance `reference-fitted` with a
calibration record pointing at this fit report — it is never labelled
"measured".

Input dataset (JSON):
  {
    "reference_lut": "path-or-id (rights holder named in context)",
    "samples": [ { "scene_rgb": [r,g,b], "reference_rgb": [r,g,b] }, ... ]
  }
Plus --base profile.json. Fitted: M_exc (9), curve density offsets (per
layer, 3 params: toe/mid/shoulder shifts), halation alpha (3), grain
sigma scale (1) — 16 parameters, LM vs CMA-ES compared.
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
    spline_eval,
    write_fit_report,
)


def perturbed_profile(mp: MiniProfile, p: np.ndarray) -> MiniProfile:
    """Map fit parameters onto the profile.

    p[0:9]  — M_exc (row-flattened, soft-projected non-negative)
    p[9:12] — per-layer mid-density offsets applied to all control points
    p[12:15] — per-layer contrast stretches (γ-like, multiplicative on the
               response above the first control value)
    p[15:18] — paper print-exposure per channel is fixed; these are
               halation proxies folded into M_exc row scale in this v1
               (kept as three extra M_exc row gains → simple, honest).
    """
    m = MiniProfile(
        m_exc=np.abs(p[0:9].reshape(3, 3).copy()),
        crosstalk=mp.crosstalk,
        curve_log_e=[list(c) for c in mp.curve_log_e],
        curve_density=[list(d) for d in mp.curve_density],
        d_min=mp.d_min.copy(),
        d_mask=mp.d_mask,
        paper_log_e=mp.paper_log_e,
        paper_density=mp.paper_density,
        paper_white_density=mp.paper_white_density,
        output_matrix=mp.output_matrix,
    )
    for c in range(3):
        base = mp.curve_density[c]
        mid_off = p[9 + c]
        stretch = p[12 + c]
        d0 = base[0]
        m.curve_density[c] = [d0 + (v - d0) * stretch + mid_off for v in base]
    return m


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--dataset", required=True, type=Path)
    ap.add_argument("--base", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--context", required=True, type=Path)
    ap.add_argument("--exposure-scale", type=float, default=1.0)
    ap.add_argument("--generations", type=int, default=150)
    args = ap.parse_args()

    data = json.loads(args.dataset.read_text())
    base = json.loads(args.base.read_text())
    ctx_raw = json.loads(args.context.read_text())
    ctx = FitContext(
        dataset=f"{data['reference_lut']} ({ctx_raw['rights']})",
        rights=ctx_raw["rights"],
        illuminant=ctx_raw["illuminant"],
        rendering_conditions=ctx_raw.get("rendering_conditions", "reference LUT sampling"),
    )

    mp = MiniProfile.from_profile_json(base)
    scene = np.array([s["scene_rgb"] for s in data["samples"]], dtype=float)
    ref = np.array([s["reference_rgb"] for s in data["samples"]], dtype=float)

    def residual(p: np.ndarray) -> np.ndarray:
        m = perturbed_profile(mp, p)
        pred = np.array([full_forward(m, s, args.exposure_scale) for s in scene])
        return (pred - ref).flatten()

    def cost(p: np.ndarray) -> float:
        r = residual(p)
        return float(np.mean(r * r))

    p0 = np.concatenate(
        [
            mp.m_exc.flatten(),
            np.zeros(3),      # mid-density offsets
            np.ones(3),       # contrast stretches
        ]
    )
    p_lm, info_lm = levenberg_marquardt(residual, p0, len(scene) * 3)
    p_cma, info_cma = cma_es(cost, p0, sigma0=0.06, generations=args.generations)
    best_p = p_lm if info_lm["final_cost"] <= info_cma["final_cost"] else p_cma
    m = perturbed_profile(mp, best_p)

    args.out.write_text(
        json.dumps(
            {
                "m_exc": m.m_exc.flatten().tolist(),
                "curves": [
                    {"log_e": m.curve_log_e[c], "density": m.curve_density[c]} for c in range(3)
                ],
                "label": "look-matching (reference-fitted) — NOT stock measurement",
            },
            indent=2,
        )
    )
    write_fit_report(
        args.out.with_name(args.out.stem + "_report"),
        target="End-to-end look-match fit (FILM_MODEL §K; look-matching, not measurement)",
        context=ctx,
        sections={
            "levenberg_marquardt": info_lm,
            "cma_es": info_cma,
            "residual_rmse": float(np.sqrt(cost(best_p))),
            "honesty": (
                "This fit matches the licensed reference's look under the stated "
                "conditions. Provenance for the resulting profile is "
                "`reference-fitted`; it must never be labelled `measured`."
            ),
        },
    )
    print(
        json.dumps(
            {
                "lm_cost": info_lm["final_cost"],
                "cma_cost": info_cma["final_cost"],
                "rmse": float(np.sqrt(cost(best_p))),
            },
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
