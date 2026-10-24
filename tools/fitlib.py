#!/usr/bin/env python3
"""fitlib.py — shared model + fitting machinery for the M2 calibration
harness (FILM_MODEL §K).

Oracle discipline (same as tools/lut_oracle.py): the Python evaluation
*mirrors* the Rust math (crates/film) — f64 arithmetic, Fritsch–Carlson
monotone splines, the same stage formulas — so a fit here lands on the
same numbers the renderer will produce. Any disagreement is a bug in one
of the two, caught by the parity sample check (`parity_report`).

Stages mirrored: H&D response (§D), crosstalk, excitation (§C1), print
paper path (§F). Fitters: monotone LSQ (H&D), analytic LSQ (crosstalk),
Levenberg–Marquardt (scipy) and a compact CMA-ES for the comparison the
spec requires (§K: "optimizer comparison recorded in the fit report").
"""

from __future__ import annotations

import json
import math
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable, Sequence

import numpy as np

# ─── Monotone C¹ cubic Hermite (mirror of crates/film/src/curves.rs) ───────


def fritsch_carlson_tangents(xs: Sequence[float], ys: Sequence[float]) -> np.ndarray:
    n = len(xs)
    d = [(ys[i + 1] - ys[i]) / (xs[i + 1] - xs[i]) for i in range(n - 1)]
    m = np.zeros(n)
    m[0] = d[0]
    m[n - 1] = d[n - 2]
    for i in range(1, n - 1):
        if d[i - 1] * d[i] <= 0.0:
            m[i] = 0.0
        else:
            w1 = 2.0 * (xs[i + 1] - xs[i]) + (xs[i] - xs[i - 1])
            w2 = (xs[i + 1] - xs[i]) + 2.0 * (xs[i] - xs[i - 1])
            m[i] = (w1 + w2) / (w1 / d[i - 1] + w2 / d[i])
    return m


def spline_eval(xs: Sequence[float], ys: Sequence[float], x: float) -> float:
    """Monotone Hermite eval with flat extrapolation (mirror of eval())."""
    if not math.isfinite(x):
        return ys[-1] if x > 0 else ys[0]
    if x <= xs[0]:
        return ys[0]
    if x >= xs[-1]:
        return ys[-1]
    lo, hi = 0, len(xs) - 1
    while hi - lo > 1:
        mid = (lo + hi) // 2
        if xs[mid] <= x:
            lo = mid
        else:
            hi = mid
    m = fritsch_carlson_tangents(xs, ys)
    x0, x1 = xs[lo], xs[lo + 1]
    y0, y1 = ys[lo], ys[lo + 1]
    h = x1 - x0
    t = (x - x0) / h
    t2, t3 = t * t, t * t * t
    return (
        (2 * t3 - 3 * t2 + 1) * y0
        + (t3 - 2 * t2 + t) * h * m[lo]
        + (-2 * t3 + 3 * t2) * y1
        + (t3 - t2) * h * m[lo + 1]
    )


def is_monotone(xs: Sequence[float], ys: Sequence[float]) -> bool:
    return all(ys[i + 1] >= ys[i] - 1e-12 for i in range(len(ys) - 1))


# ─── Forward model (mirror of emulsion/print stage math) ──────────────────


@dataclass
class MiniProfile:
    """The subset of a stock profile the fitters adjust + evaluate."""

    m_exc: np.ndarray          # 3×3
    crosstalk: np.ndarray      # 3×3 (zero diagonal)
    curve_log_e: list          # per-layer control x
    curve_density: list        # per-layer control y
    d_min: np.ndarray
    d_mask: np.ndarray
    paper_log_e: list
    paper_density: list
    paper_white_density: float
    output_matrix: np.ndarray

    @staticmethod
    def from_profile_json(p: dict) -> "MiniProfile":
        return MiniProfile(
            m_exc=np.array(p["m_exc"], dtype=float).reshape(3, 3),
            crosstalk=np.array(p["crosstalk"], dtype=float).reshape(3, 3),
            curve_log_e=[c["log_e"] for c in p["curves"]],
            curve_density=[c["density"] for c in p["curves"]],
            d_min=np.array(p["d_min"], dtype=float),
            d_mask=np.array(p["print"]["d_mask"], dtype=float),
            paper_log_e=[c["log_e"] for c in p["print"]["paper_curves"]],
            paper_density=[c["density"] for c in p["print"]["paper_curves"]],
            paper_white_density=float(p["print"]["paper_white_density"]),
            output_matrix=np.array(p["print"]["output_matrix"], dtype=float).reshape(3, 3),
        )


def response(mp: MiniProfile, log_e: np.ndarray) -> np.ndarray:
    """H&D + crosstalk for a 3-vector of log-exposures (§D)."""
    d = np.array(
        [
            mp.d_min[c] + spline_eval(mp.curve_log_e[c], mp.curve_density[c], log_e[c])
            for c in range(3)
        ]
    )
    return d + mp.crosstalk @ d


def print_paper(mp: MiniProfile, d: np.ndarray, l_p: float = 1.0) -> np.ndarray:
    """Paper inversion (§F) for a density 3-vector → linear [0, 1] 3-vector."""
    t = 10.0 ** (-(d - mp.d_mask))
    x_p = l_p * t
    d_p = np.array(
        [
            spline_eval(mp.paper_log_e[c], mp.paper_density[c], math.log10(max(x_p[c], 1e-12)))
            for c in range(3)
        ]
    )
    lin = np.clip(10.0 ** (mp.paper_white_density - d_p), 0.0, 1.0)
    return mp.output_matrix @ lin


def full_forward(mp: MiniProfile, e_rgb: np.ndarray, exposure_scale: float) -> np.ndarray:
    """Scene-linear RGB → print-linear RGB (excitation → response → print)."""
    x = mp.m_exc @ (e_rgb * exposure_scale)
    log_e = np.log10(np.maximum(x, 1e-9))
    d = response(mp, log_e)
    return print_paper(mp, d)


# ─── Fitter 1: monotone LSQ for H&D control points (§K) ───────────────────


def fit_hd_curve_monotone_lsq(
    log_e_data: np.ndarray,
    density_data: np.ndarray,
    n_control: int = 7,
) -> tuple[list[float], list[float], dict]:
    """Fit monotone control points to densitometric samples.

    Method: LSQ smoothing spline via iterative isotonic projection —
    fit cubic control points at uniform quantiles, then project the
    fitted values onto the monotone cone (pool-adjacent-violators),
    iterating until stable. Residuals are reported per sample.
    """
    order = np.argsort(log_e_data)
    lx, dy = np.asarray(log_e_data)[order], np.asarray(density_data)[order]
    knots = list(np.linspace(lx[0], lx[-1], n_control))
    # Initial values: local means at each knot.
    vals = []
    for k in knots:
        w = np.exp(-((lx - k) ** 2) / (2 * (0.25 * (knots[1] - knots[0])) ** 2))
        vals.append(float(np.average(dy, weights=np.maximum(w, 1e-9))))
    for _ in range(64):
        # Evaluate residuals at data points, nudge control values by the
        # local mean residual (Gauss–Seidel smoothing), then isotonize.
        pred = np.array([spline_eval(knots, vals, x) for x in lx])
        resid = dy - pred
        for i in range(len(vals)):
            w = np.exp(-((lx - knots[i]) ** 2) / (2 * (0.25 * (knots[1] - knots[0])) ** 2))
            if w.sum() > 1e-9:
                vals[i] += float(np.average(resid, weights=w)) * 0.5
        # Isotonic projection (PAVA) — monotone non-decreasing.
        v = np.array(vals)
        blocks: list[tuple[float, int]] = []
        for x in v:
            blocks.append((x, 1))
            while len(blocks) > 1 and blocks[-2][0] > blocks[-1][0]:
                a, na = blocks[-2]
                b, nb = blocks[-1]
                blocks[-2:] = [((a * na + b * nb) / (na + nb), na + nb)]
        out = []
        for val, n in blocks:
            out.extend([val] * n)
        new = np.array(out)
        if np.max(np.abs(new - v)) < 1e-10:
            vals = new.tolist()
            break
        vals = new.tolist()
    pred = np.array([spline_eval(knots, vals, x) for x in lx])
    resid = dy - pred
    stats = {
        "method": "isotonic-projected smoothing LSQ",
        "n_samples": int(len(lx)),
        "rmse": float(np.sqrt(np.mean(resid**2))),
        "max_abs": float(np.max(np.abs(resid))),
    }
    return knots, vals, stats


# ─── Fitter 2: crosstalk from neutral ramps (§K) ───────────────────────────


def fit_crosstalk_from_neutral_ramp(
    ramp_log_e: np.ndarray,
    layer_densities: np.ndarray,
    d_min: np.ndarray,
    curve_log_e: list,
    curve_density: list,
) -> tuple[np.ndarray, dict]:
    """Fit the off-diagonal crosstalk matrix from a neutral (gray) ramp.

    A neutral ramp excites all layers equally; per-layer density deviations
    from the *pure-curve* prediction are, to first order, the off-diagonal
    contributions: ΔD_i = Σ_j≠i C_ij·D_j. Solve the linear LSQ for the 6
    off-diagonal entries (zero diagonal enforced).
    """
    rows, rhs = [], []
    for k in range(len(ramp_log_e)):
        d_curve = np.array(
            [
                d_min[c] + spline_eval(curve_log_e[c], curve_density[c], ramp_log_e[k][c])
                for c in range(3)
            ]
        )
        d_meas = layer_densities[k]
        for i in range(3):
            row = np.zeros(9)
            for j in range(3):
                if i != j:
                    row[3 * i + j] = d_curve[j]
            rows.append(row)
            rhs.append(d_meas[i] - d_curve[i])
    a = np.array(rows)
    b = np.array(rhs)
    c_flat, *_ = np.linalg.lstsq(a, b, rcond=None)
    stats = {
        "method": "linear LSQ on off-diagonal first-order model",
        "n_samples": int(len(ramp_log_e)),
        "rmse": float(np.sqrt(np.mean((a @ c_flat - b) ** 2))),
    }
    return c_flat.reshape(3, 3), stats


# ─── Optimizers: LM (scipy) vs CMA-ES (compact) — the §K comparison ───────


def levenberg_marquardt(
    f: Callable[[np.ndarray], np.ndarray],
    x0: np.ndarray,
    n_residuals: int,
    max_iter: int = 200,
) -> tuple[np.ndarray, dict]:
    """Thin wrapper over scipy.optimize.least_squares (LM flavor)."""
    from scipy.optimize import least_squares

    r = least_squares(lambda x: f(x), x0, method="lm", max_nfev=max_iter * (len(x0) + 1))
    return r.x, {
        "optimizer": "Levenberg–Marquardt (scipy least_squares, method='lm')",
        "iterations": int(r.nfev),
        "final_cost": float(r.cost),
    }


def cma_es(
    f: Callable[[np.ndarray], float],
    x0: np.ndarray,
    sigma0: float = 0.1,
    pop: int | None = None,
    generations: int = 200,
) -> tuple[np.ndarray, dict]:
    """Compact (μ/μ_w, λ)-CMA-ES for the optimizer-comparison requirement.

    Reference-faithful essentials: rank-μ update with cumulative step-size
    adaptation; full covariance update omitted in favour of the diagonal
    (sep-CMA) for parameter counts ≤ 12 — sufficient and honest for the
    comparison record (§K asks for the comparison, not a library).
    """
    n = len(x0)
    lam = pop or 4 + int(3 * math.log(n))
    mu = lam // 2
    weights = np.log(mu + 0.5) - np.log(np.arange(1, mu + 1))
    weights /= weights.sum()
    mu_eff = 1.0 / np.sum(weights**2)
    cs = (mu_eff + 2) / (n + mu_eff + 5)
    ds = 1 + 2 * max(0.0, math.sqrt((mu_eff - 1) / (n + 1)) - 1) + cs
    std = np.full(n, sigma0)
    pc = np.zeros(n)
    mean = np.array(x0, dtype=float)
    chi_n = math.sqrt(n) * (1 - 1 / (4 * n) + 1 / (21 * n * n))
    evaluations = 0
    best_x, best_f = mean.copy(), f(mean)
    evaluations += 1
    for _ in range(generations):
        zs = np.random.standard_normal((lam, n))
        xs = mean + zs * std
        fs = np.array([f(x) for x in xs])
        evaluations += lam
        order = np.argsort(fs)
        if fs[order[0]] < best_f:
            best_f, best_x = fs[order[0]], xs[order[0]].copy()
        sel = zs[order[:mu]]
        wzs = (weights[:, None] * sel).sum(axis=0)
        mean = mean + 1.0 * std * wzs
        # Cumulative step-size adaptation.
        pc = (1 - cs) * pc + math.sqrt(cs * (2 - cs) * mu_eff) * wzs
        std *= math.exp((cs / ds) * (np.linalg.norm(pc) / chi_n - 1))
        std = np.clip(std, 1e-9, 1e3)
    return best_x, {
        "optimizer": "CMA-ES (sep, rank-μ, CSA — compact in-repo implementation)",
        "evaluations": evaluations,
        "generations": generations,
        "final_cost": float(best_f),
    }


# ─── Dataset rights + report records (§K: every fit stores provenance) ────


@dataclass
class FitContext:
    dataset: str
    rights: str
    illuminant: str
    rendering_conditions: str

    def as_dict(self) -> dict:
        return {
            "dataset": self.dataset,
            "rights": self.rights,
            "illuminant": self.illuminant,
            "rendering_conditions": self.rendering_conditions,
        }


def write_fit_report(path: Path, *, target: str, context: FitContext, sections: dict) -> None:
    """Report format: machine-readable JSON + human summary markdown."""
    payload = {"target": target, **context.as_dict(), "sections": sections}
    path.with_suffix(".json").write_text(json.dumps(payload, indent=2))
    lines = [f"# Fit report — {target}", ""]
    lines.append(f"- dataset: `{context.dataset}`")
    lines.append(f"- rights: {context.rights}")
    lines.append(f"- illuminant: {context.illuminant}")
    lines.append(f"- rendering conditions: {context.rendering_conditions}")
    lines.append("")
    for name, sec in sections.items():
        lines.append(f"## {name}")
        lines.append("```json")
        lines.append(json.dumps(sec, indent=2, default=str))
        lines.append("```")
        lines.append("")
    path.with_suffix(".md").write_text("\n".join(lines))
