#!/usr/bin/env python3
"""Generate the M3 dummy parameter-predictor ONNX fixture (issue #14).

A deliberately naive but *real* parameter predictor — a linear gray-world
model expressed in plain ONNX ops (opset 17):

    input  [1,3,64,64] f32  sRGB-display encoded 0..1 (NCHW)
    means  = GlobalAveragePool(input)              # [1,3,1,1]
    m      = Squeeze(means)                        # [1,3] (r,g,b)
    Y      = 0.2722*r + 0.6741*g + 0.0537*b        # luma (approx AP1 weights)
    ev     = log2(0.4610 / Y)                      # sRGB(0.18) in display space
    wb     = [ m_g/m_r , 1.0 , m_g/m_b ]           # gray-world gains (green-normalized)
    output [1,6] = [ ev , wb_r , wb_g , wb_b , 0 , 0 ]

No percentile statistics, no highlight guard, no skin guard, no trimmed
means, no OETF decode before the gain ratios — exactly the naivety the
§2 baseline is designed to beat. It exists to exercise the full
vet→load→preprocess→infer→decode→benchmark pipeline with zero third-party
weights (ADR-0004 §4: "any packaged script in a model repo is data" —
this one is ours, co-located in the project repo).

Provenance: authored by the MELOGRAPHY project; the generator script and
the committed model are co-located in this repository. License:
GPL-3.0-or-later (code); the model artifact itself is CC0-1.0
(project-generated data — no third-party weights involved).

Usage: python3 scripts/gen_dummy_predictor.py [--out PATH]
"""

import argparse
import hashlib
import json
from pathlib import Path

import onnx
from onnx import TensorProto, helper, numpy_helper
import numpy as np

OPSET = 17


def build_model() -> onnx.ModelProto:
    x = helper.make_tensor_value_info("input", TensorProto.FLOAT, [1, 3, 64, 64])
    y = helper.make_tensor_value_info("output", TensorProto.FLOAT, [1, 6])

    nodes = [
        # means: [1,3,1,1]
        helper.make_node("GlobalAveragePool", ["input"], ["means"]),
        # m: [1,3] (squeeze the trailing 1×1)
        helper.make_node(
            "Squeeze", ["means", "axes_23"], ["m"], name="squeeze"
        ),
        # Channel split via Gather on axis 1.
        helper.make_node("Gather", ["m", "idx_r"], ["mr"], axis=1),
        helper.make_node("Gather", ["m", "idx_g"], ["mg"], axis=1),
        helper.make_node("Gather", ["m", "idx_b"], ["mb"], axis=1),
        # Luma: Y = 0.2722*r + 0.6741*g + 0.0537*b  (broadcast scalars [1,1])
        helper.make_node("Mul", ["mr", "w_r"], ["r1"]),
        helper.make_node("Mul", ["mg", "w_g"], ["r2"]),
        helper.make_node("Mul", ["mb", "w_b"], ["r3"]),
        helper.make_node("Add", ["r1", "r2"], ["r12"]),
        helper.make_node("Add", ["r12", "r3"], ["y_mean"]),  # [1,1]
        # ev = log2(srgb(0.18) / Y) = (log(0.4610) - log(Y)) / log(2)
        # (input is display-encoded: 18 % gray sits at ≈ 0.4610 there)
        helper.make_node("Log", ["y_mean"], ["log_y"]),
        helper.make_node("Sub", ["log_target", "log_y"], ["log_ratio"]),
        helper.make_node("Div", ["log_ratio", "log_2"], ["ev"]),  # [1,1]
        # Gray-world gains (green-normalized): gr = mg/mr, gg = 1, gb = mg/mb
        helper.make_node("Div", ["mg", "mr"], ["gain_r"]),
        helper.make_node("Div", ["mg", "mb"], ["gain_b"]),
        # highlights = shadows = 0
        helper.make_node("Mul", ["ev", "zero"], ["hl"]),
        helper.make_node("Mul", ["ev", "zero"], ["sh"]),
        # output: concat [ev, gain_r, one, gain_b, hl, sh] → [1,6]
        helper.make_node(
            "Concat", ["ev", "gain_r", "one", "gain_b", "hl", "sh"], ["output"], axis=1
        ),
    ]

    def t_f32(name, vals, dims):
        return numpy_helper.from_array(np.array(vals, dtype=np.float32).reshape(dims), name)

    def t_i64(name, vals):
        return numpy_helper.from_array(np.array(vals, dtype=np.int64), name)

    initializers = [
        t_i64("axes_23", [2, 3]),
        t_i64("idx_r", [0]),
        t_i64("idx_g", [1]),
        t_i64("idx_b", [2]),
        t_f32("w_r", [0.2722287], [1, 1]),
        t_f32("w_g", [0.6740818], [1, 1]),
        t_f32("w_b", [0.0536895], [1, 1]),
        t_f32("log_target", [float(np.log(0.4610))], [1, 1]),
        t_f32("log_2", [float(np.log(2.0))], [1, 1]),
        t_f32("one", [1.0], [1, 1]),
        t_f32("zero", [0.0], [1, 1]),
    ]

    graph = helper.make_graph(
        nodes, "melography_dummy_param_predictor", [x], [y], initializer=initializers
    )
    model = helper.make_model(
        graph,
        producer_name="melography/scripts/gen_dummy_predictor.py",
        opset_imports=[helper.make_opsetid("", OPSET)],
    )
    model.ir_version = 8
    return model


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument(
        "--out",
        default="crates/ai/tests/fixtures/dummy_param_predictor.onnx",
    )
    args = ap.parse_args()
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    model = build_model()
    onnx.checker.check_model(model, full_check=True)
    data = model.SerializeToString()
    out.write_bytes(data)
    sha = hashlib.sha256(data).hexdigest()
    prov = {
        "id": "melography-dummy-param-predictor",
        "version": "1.0.0",
        "generated_by": "scripts/gen_dummy_predictor.py",
        "weights_origin": "project-generated constants (no third-party weights)",
        "license": "CC0-1.0 (project-generated data)",
        "sha256": sha,
        "size_bytes": len(data),
        "opset": OPSET,
        "note": "evaluation-harness plumbing proof + naive gray-world reference competitor for the issue #14 benchmark; NOT a shipping candidate",
    }
    prov_path = out.with_suffix(".provenance.json")
    prov_path.write_text(json.dumps(prov, indent=2) + "\n")
    print(f"wrote {out} ({len(data)} bytes)")
    print(f"sha256 {sha}")
    print(f"provenance {prov_path}")


if __name__ == "__main__":
    main()
