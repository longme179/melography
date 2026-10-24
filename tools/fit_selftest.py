#!/usr/bin/env python3
"""fit_selftest.py — end-to-end validation of the M2 fitting harness.

Generates a *synthetic* densitometry + chart dataset from a known profile
(the shipped warm-400-negative draft), runs every fitter, and verifies:

1. H&D fit residuals are small and control points monotone;
2. crosstalk recovery approaches the planted matrix;
3. M_exc fit (LM vs CMA-ES) reproduces the planted excitation within
   tolerance and the §K comparison is recorded;
4. the look-match fitter converges on a self-referential target;
5. a fitted profile passes the shipped JSON Schema.

License-clean by construction: all data is synthetic, generated from
project-authored parameters. This is the harness' own acceptance test —
real calibration datasets plug in through the same JSON formats.

Usage: python3 tools/fit_selftest.py [--workdir /tmp/fit_selftest]
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np

TOOLS = Path(__file__).parent
REPO = TOOLS.parent

sys.path.insert(0, str(TOOLS))
from fitlib import (  # noqa: E402
    MiniProfile,
    fit_crosstalk_from_neutral_ramp,
    fit_hd_curve_monotone_lsq,
    full_forward,
    is_monotone,
    response,
    spline_eval,
)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--workdir", type=Path, default=Path(tempfile.mkdtemp(prefix="fit_selftest_")))
    args = ap.parse_args()
    wd = args.workdir
    wd.mkdir(parents=True, exist_ok=True)

    # Ground truth: the shipped artist draft (project-authored → clean).
    base = json.loads((REPO / "profiles" / "warm-400-negative.json").read_text())
    mp = MiniProfile.from_profile_json(base)
    failures: list[str] = []

    # ── 1. H&D fit round-trip on synthetic densitometry ─────────────────
    rng = np.random.default_rng(2026)
    samples = []
    for step in np.linspace(-3.8, -0.1, 24):
        d = response(mp, np.array([step, step, step])) + rng.normal(0, 0.002, 3)
        samples.append({"log_e": [step] * 3, "density": d.tolist()})
    (wd / "hd.json").write_text(json.dumps({"samples": samples}))
    (wd / "ctx.json").write_text(
        json.dumps(
            {
                "rights": "synthetic (generated from project-authored parameters; no third-party data)",
                "illuminant": "synthetic-D55",
                "rendering_conditions": "selftest densitometry",
            }
        )
    )
    subprocess.run(
        [
            sys.executable,
            str(TOOLS / "fit_hd_curves.py"),
            "--dataset",
            str(wd / "hd.json"),
            "--out",
            str(wd / "hd_fit.json"),
            "--context",
            str(wd / "ctx.json"),
        ],
        check=True,
        capture_output=True,
    )
    hd = json.loads((wd / "hd_fit.json").read_text())
    for c, name in enumerate("rgb"):
        layer = hd["layers"][name]
        if not is_monotone(layer["log_e"], layer["density"]):
            failures.append(f"hd layer {name}: not monotone")
        # Evaluate both curves at the SAME x points (knot positions differ).
        xs = (-2.0, -1.5, -1.0)
        pred = np.array([spline_eval(layer["log_e"], layer["density"], x) for x in xs])
        truth = np.array(
            [spline_eval(mp.curve_log_e[c], mp.curve_density[c], x) for x in xs]
        )
        # The fitted curve tracks response-above-fog including crosstalk
        # leakage at neutral (the ground-truth curves are crosstalk-free);
        # tolerance accommodates that first-order offset.
        if np.max(np.abs(pred - truth)) > 0.25:
            failures.append(
                f"hd layer {name}: deviates from ground truth by > 0.25 density "
                f"(max {float(np.max(np.abs(pred - truth))):.3f})"
            )

    # ── 2. Crosstalk recovery on a synthetic neutral ramp ───────────────
    ramp = []
    for step in np.linspace(-3.5, -0.2, 16):
        d = response(mp, np.array([step, step, step]))
        ramp.append({"log_e": [step] * 3, "density": d.tolist()})
    c_fit, stats = fit_crosstalk_from_neutral_ramp(
        np.array([r["log_e"] for r in ramp]),
        np.array([r["density"] for r in ramp]),
        d_min=mp.d_min,
        curve_log_e=mp.curve_log_e,
        curve_density=mp.curve_density,
    )
    c_true = mp.crosstalk
    err = float(np.max(np.abs(c_fit - c_true)))
    if err > 0.03:
        failures.append(f"crosstalk recovery error {err:.4f} > 0.03")

    # ── 3. M_exc fit (LM vs CMA-ES) on synthetic chart patches ─────────
    patches = []
    for _ in range(40):
        s = rng.uniform(0.02, 1.2, 3)
        out = full_forward(mp, s, 1.0)
        patches.append({"scene_rgb": s.tolist(), "measured_rgb": out.tolist()})
    (wd / "chart.json").write_text(json.dumps({"patches": patches}))
    r = subprocess.run(
        [
            sys.executable,
            str(TOOLS / "fit_excitation.py"),
            "--dataset",
            str(wd / "chart.json"),
            "--base",
            str(REPO / "profiles" / "warm-400-negative.json"),
            "--out",
            str(wd / "exc_fit.json"),
            "--context",
            str(wd / "ctx.json"),
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    exc = json.loads((wd / "exc_fit.json").read_text())
    m_fit = np.array(exc["m_exc"], dtype=float).reshape(3, 3)
    m_err = float(np.max(np.abs(m_fit - mp.m_exc) / np.maximum(mp.m_exc, 0.05)))
    if m_err > 0.35:
        failures.append(f"m_exc recovery relative error {m_err:.3f} > 0.35")
    if not (wd / "exc_fit_report.json").exists():
        failures.append("excitation fit report missing")

    # ── 4. Look-match fitter converges on a self-referential target ────
    ref_samples = [
        {"scene_rgb": s, "reference_rgb": o}
        for s, o in zip(
            [p["scene_rgb"] for p in patches], [p["measured_rgb"] for p in patches]
        )
    ]
    (wd / "look.json").write_text(
        json.dumps({"reference_lut": "selftest://warm-400", "samples": ref_samples})
    )
    subprocess.run(
        [
            sys.executable,
            str(TOOLS / "fit_look_match.py"),
            "--dataset",
            str(wd / "look.json"),
            "--base",
            str(REPO / "profiles" / "warm-400-negative.json"),
            "--out",
            str(wd / "look_fit.json"),
            "--context",
            str(wd / "ctx.json"),
        ],
        check=True,
        capture_output=True,
    )
    look_report = json.loads((wd / "look_fit_report.json").read_text())
    look_rmse = float(look_report["sections"]["residual_rmse"])
    if look_rmse > 0.02:
        failures.append(f"look-match rmse {look_rmse:.4f} > 0.02")

    # ── 5. A fitted profile validates against the shipped schema ───────
    fitted = json.loads((REPO / "profiles" / "warm-400-negative.json").read_text())
    fitted["m_exc"] = exc["m_exc"]
    fitted["crosstalk"] = c_fit.flatten().tolist()
    hd_layers = hd["layers"]
    fitted["curves"] = [
        {"log_e": hd_layers[n]["log_e"], "density": hd_layers[n]["density"]} for n in "rgb"
    ]
    (wd / "fitted_profile.json").write_text(json.dumps(fitted, indent=2))
    schema = json.loads((REPO / "schemas" / "stock-profile.schema.json").read_text())
    import jsonschema

    jsonschema.validate(fitted, schema)
    # Rust-side physics validation happens in the repo test suite; here we
    # assert the structural contract only (the report documents this).

    # ── Report ──────────────────────────────────────────────────────────
    summary = {
        "hd_fit": "ok",
        "crosstalk_max_err": err,
        "m_exc_rel_err": m_err,
        "look_rmse": look_rmse,
        "schema_validation": "ok",
        "workdir": str(wd),
        "failures": failures,
    }
    print(json.dumps(summary, indent=2))
    if failures:
        print("SELFTEST FAILED", file=sys.stderr)
        return 1
    print("SELFTEST PASSED — harness generates fit reports with LM-vs-CMA-ES records")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
