#!/usr/bin/env python3
"""Independent tetrahedral-interpolation oracle for MELOGRAPHY (issue #7).

Purpose
-------
`crates/lut` contains the production CPU implementation. This script is the
*independent reference* required by `docs/research/luts.md` §2: it is
written from the geometry of the problem (sort the fractional coordinates,
build the containing tetrahedron, solve its barycentric weights) — NOT by
transcribing the Rust code. Both sides must agree to <= 1e-6 on the shared
fixture set (`crates/lut/tests/oracle_fixtures.json`); CI regenerates the
fixture and diffs it so the oracle can never silently rot.

Usage
-----
    python3 tools/lut_oracle.py <fixture.json>        # (re)generate
    python3 tools/lut_oracle.py --verify <fixture.json>   # regenerate to
                                                       # memory + diff

Exit codes: 0 ok, 1 mismatch/missing, 2 usage error.

Only the Python standard library is used (no numpy) so every CI runner and
the dev sandbox can run it unchanged.

Tetrahedral math (independent derivation)
-----------------------------------------
The unit cell of the LUT lattice splits into 6 tetrahedra that share the
main-diagonal edge (0,0,0)-(1,1,1). Given fractions f = (fx,fy,fz), sort the
axes by descending fraction: order = (p0, p1, p2). The containing
tetrahedron's vertices are the path

    v0 = (0,0,0), v1 = v0 + e_{p0}, v2 = v1 + e_{p1}, v3 = (1,1,1)

and the barycentric weights are

    w0 = 1 - f[p0],  w1 = f[p0] - f[p1],  w2 = f[p1] - f[p2],  w3 = f[p2]

(check: component p0 sums to f[p0], p1 to f[p1], p2 to f[p2], total = 1).
The interpolated value is sum(w_i * corner(v_i)). Inputs clamp to the
declared domain first; clamp flags are recorded per channel.
"""

import json
import random
import sys

VERSION = "melography.lut_oracle.v1"
TOLERANCE_NOTE = "rust-vs-oracle max |delta| must be <= 1e-6"


def tetra_sample(size, domain_min, domain_max, table, rgb):
    """Sample one 3D LUT value. `table` is red-fastest: idx (i,j,k) at
    (i + j*N + k*N*N)*3. Returns (out, clamped_flags)."""
    n = size
    last = n - 1
    out = [0.0, 0.0, 0.0]
    clamped = [False, False, False]
    v = [0.0, 0.0, 0.0]
    for c in range(3):
        x = rgb[c]
        lo, hi = domain_min[c], domain_max[c]
        if x < lo:
            v[c] = lo
            clamped[c] = True
        elif x > hi:
            v[c] = hi
            clamped[c] = True
        else:
            v[c] = x
    g = [(v[c] - domain_min[c]) / (domain_max[c] - domain_min[c]) * last
         for c in range(3)]
    i0 = [min(int(g[c] // 1), n - 2) for c in range(3)]
    f = [g[c] - i0[c] for c in range(3)]

    def corner(di, dj, dk):
        base = ((i0[0] + di) + (i0[1] + dj) * n + (i0[2] + dk) * n * n) * 3
        return (table[base], table[base + 1], table[base + 2])

    # Sort axes by descending fraction (stable → deterministic on ties).
    order = sorted(range(3), key=lambda c: -f[c])
    verts = [(0, 0, 0)]
    cur = [0, 0, 0]
    for c in order:
        cur = list(cur)
        cur[c] = 1
        verts.append(tuple(cur))
    fp = [f[c] for c in order]
    w = [1.0 - fp[0], fp[0] - fp[1], fp[1] - fp[2], fp[2]]
    for wi, vert in zip(w, verts):
        cn = corner(*vert)
        for ch in range(3):
            out[ch] += wi * cn[ch]
    return out, clamped


def one_d_sample(size, domain_min, domain_max, table, rgb):
    """Per-channel piecewise-linear 1D semantics (row i, column ch)."""
    n = size
    last = n - 1
    out = [0.0, 0.0, 0.0]
    clamped = [False, False, False]
    for ch in range(3):
        lo, hi = domain_min[ch], domain_max[ch]
        x = rgb[ch]
        if x < lo:
            x = lo
            clamped[ch] = True
        elif x > hi:
            x = hi
            clamped[ch] = True
        g = (x - lo) / (hi - lo) * last
        i0 = min(int(g // 1), n - 2)
        fr = g - i0
        out[ch] = table[i0 * 3 + ch] * (1.0 - fr) + table[(i0 + 1) * 3 + ch] * fr
    return out, clamped


def cube_text(kind, size, domain_min, domain_max, table, title):
    """Serialize to .cube text (red-fastest, %.9g floats)."""
    lines = [f'TITLE "{title}"']
    lines.append(f"LUT_{'1D' if kind == '1d' else '3D'}_SIZE {size}")
    lines.append("DOMAIN_MIN " + " ".join(f"{v:.9g}" for v in domain_min))
    lines.append("DOMAIN_MAX " + " ".join(f"{v:.9g}" for v in domain_max))
    for i in range(0, len(table), 3):
        lines.append(" ".join(f"{v:.9g}" for v in table[i:i + 3]))
    return "\n".join(lines) + "\n"


# ─── fixture LUTs (deterministic; regenerated identically every run) ────────

def make_identity_33():
    n = 33
    table = []
    for k in range(n):
        for j in range(n):
            for i in range(n):
                table += [i / (n - 1), j / (n - 1), k / (n - 1)]
    return ("identity_33", "3d", n, [0.0] * 3, [1.0] * 3, table)


def make_crosstalk_33():
    """Smooth non-symmetric LUT: channel mixing + per-channel curves."""
    n = 33
    table = []
    for k in range(n):
        for j in range(n):
            for i in range(n):
                r, g, b = i / (n - 1), j / (n - 1), k / (n - 1)
                rr = 0.29 * r + 0.51 * g + 0.18 * b
                gg = 0.21 * r + 0.67 * g + 0.11 * b
                bb = 0.31 * r + 0.09 * g + 0.58 * b
                # gentle S-curve per output channel (still smooth)
                table += [x * x * (3 - 2 * x) for x in (rr, gg, bb)]
    return ("crosstalk_33", "3d", n, [0.0] * 3, [1.0] * 3, table)


def make_extreme_corners_2():
    """N=2 with extreme asymmetric corner values (all 6 tetra paths active
    across the sample set)."""
    table = []
    corners = [
        (0.00, 0.10, 0.90), (1.30, -0.20, 0.40),
        (0.25, 1.40, 0.15), (0.85, 0.95, 1.10),
        (-0.30, 0.05, 0.60), (0.70, 0.35, -0.40),
        (0.45, -0.50, 1.25), (1.15, 1.05, 0.55),
    ]
    for c in corners:
        table += list(c)
    return ("extreme_corners_2", "3d", 2, [0.0] * 3, [1.0] * 3, table)


def make_wide_domain_17():
    """Non-unit domain [-0.25, 1.5] with a warm shift."""
    n = 17
    dmin, dmax = [-0.25] * 3, [1.5] * 3

    def u(i):
        return dmin[0] + (dmax[0] - dmin[0]) * i / (n - 1)

    table = []
    for k in range(n):
        for j in range(n):
            for i in range(n):
                r, g, b = u(i), u(j), u(k)
                table += [0.9 * r + 0.06, 0.92 * g, 0.85 * b - 0.02]
    return ("wide_domain_17", "3d", n, dmin, dmax, table)


def make_shaper_1d_25():
    """1D shaper: R lifts shadows, G identity, B gentle log-ish curve."""
    n = 25
    table = []
    for i in range(n):
        x = i / (n - 1)
        table += [x ** 0.85, x, (2 ** (x * 4) - 1) / 15.0]
    return ("shaper_1d_25", "1d", n, [0.0] * 3, [1.0] * 3, table)


def sample_points(kind, size, dmin, dmax, rng):
    """Boundary-critical + random sample inputs in declared-domain units."""
    lo, hi = dmin[0], dmax[0]
    pts = []
    # exact lattice points (a subset)
    if kind == "3d":
        for _ in range(24):
            i = rng.randrange(size)
            j = rng.randrange(size)
            k = rng.randrange(size)
            pts.append([dmin[c] + (dmax[c] - dmin[c]) * idx / (size - 1)
                        for c, idx in ((0, i), (1, j), (2, k))])
    else:
        for _ in range(12):
            i = rng.randrange(size)
            x = dmin[0] + (dmax[0] - dmin[0]) * i / (size - 1)
            pts.append([x, x, x])
    # cell midpoints and quarter points
    for _ in range(30):
        cell = [rng.randrange(size - 1) for _ in range(3 if kind == "3d" else 1)]
        frac = [rng.choice([0.25, 0.5, 0.75]) for _ in cell]
        if kind == "3d":
            pts.append([dmin[c] + (dmax[c] - dmin[c]) * (cell[c] + frac[c]) / (size - 1)
                        for c in range(3)])
        else:
            x = dmin[0] + (dmax[0] - dmin[0]) * (cell[0] + frac[0]) / (size - 1)
            pts.append([x, x, x])
    # pure random
    for _ in range(60):
        pts.append([dmin[c] + (dmax[c] - dmin[c]) * rng.random() for c in range(3)])
    # domain edges and just-outside (clamp flags)
    for c in range(3):
        for x in (lo - 0.05, lo, lo + 1e-6, hi - 1e-6, hi, hi + 0.05):
            v = [0.5 * (lo + hi)] * 3
            v[c] = x
            pts.append(v)
    # the all-corners set for tiny LUTs
    if kind == "3d" and size == 2:
        for i in (lo, hi):
            for j in (lo, hi):
                for k in (lo, hi):
                    pts.append([i, j, k])
    return pts


def build_fixture():
    rng = random.Random(20261024)  # fixed seed: byte-stable fixture
    specs = [
        make_identity_33(),
        make_crosstalk_33(),
        make_extreme_corners_2(),
        make_wide_domain_17(),
        make_shaper_1d_25(),
    ]
    luts = []
    for name, kind, size, dmin, dmax, table in specs:
        samples = []
        for pt in sample_points(kind, size, dmin, dmax, rng):
            if kind == "3d":
                out, clamped = tetra_sample(size, dmin, dmax, table, pt)
            else:
                out, clamped = one_d_sample(size, dmin, dmax, table, pt)
            samples.append({
                "in": pt,
                "out": out,
                "clamped": clamped,
            })
        luts.append({
            "name": name,
            "kind": kind,
            "size": size,
            "domain_min": dmin,
            "domain_max": dmax,
            # NOTE: no `table` field — the samples (computed from the
            # oracle's own red-fastest table) prove parse+layout+tetra
            # end-to-end on the Rust side, halving the fixture size.
            "cube_text": cube_text(kind, size, dmin, dmax, table, name),
            "samples": samples,
        })
    return {
        "generator": VERSION,
        "tolerance": TOLERANCE_NOTE,
        "luts": luts,
    }


def main(argv):
    if len(argv) == 3 and argv[1] == "--verify":
        path = argv[2]
        try:
            with open(path, "r", encoding="utf-8") as f:
                current = f.read()
        except OSError as e:
            print(f"FAIL: cannot read fixture {path}: {e}", file=sys.stderr)
            return 1
        regenerated = json.dumps(build_fixture(), indent=1, sort_keys=False,
                                 ensure_ascii=False) + "\n"
        if current == regenerated:
            print(f"OK: {path} matches the oracle regeneration "
                  f"({len(regenerated)} bytes)")
            return 0
        print("FAIL: fixture differs from oracle regeneration — regenerate "
              "with: python3 tools/lut_oracle.py " + path, file=sys.stderr)
        return 1
    if len(argv) == 2 and not argv[1].startswith("-"):
        fixture = build_fixture()
        with open(argv[1], "w", encoding="utf-8") as f:
            json.dump(fixture, f, indent=1, sort_keys=False, ensure_ascii=False)
            f.write("\n")
        n_samples = sum(len(l["samples"]) for l in fixture["luts"])
        print(f"wrote {argv[1]}: {len(fixture['luts'])} LUTs, "
              f"{n_samples} samples")
        return 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
