#!/usr/bin/env python3
"""Generate the S3 spike dummy ONNX model.

The model is intentionally trivial (elementwise `Y = X + 0.25` on a
[1, 3, 64, 64] f32 tensor): Spike S3 measures ONNX Runtime packaging and
invocation overhead (cold init, session creation, tensor copy, invoke) on
each OS — not model compute. M3's real models are small parameter
predictors; compute cost is measured separately when they exist.

Provenance: this file is authored by the MELOGRAPHY project (script +
committed model are co-located); no third-party weights are involved.
License: GPL-3.0-or-later, same as the project (see
spikes/s3-ort/models/provenance.json).

Usage: python3 scripts/gen_dummy_onnx.py [--out PATH]
"""

import argparse
import hashlib
from pathlib import Path

import onnx
from onnx import TensorProto, helper


def build_model() -> onnx.ModelProto:
    x = helper.make_tensor_value_info("input", TensorProto.FLOAT, [1, 3, 64, 64])
    y = helper.make_tensor_value_info("output", TensorProto.FLOAT, [1, 3, 64, 64])
    bias = helper.make_tensor(
        name="bias", data_type=TensorProto.FLOAT, dims=[1], vals=[0.25]
    )
    node = helper.make_node("Add", inputs=["input", "bias"], outputs=["output"])
    graph = helper.make_graph(
        [node], "dummy_identity_plus_bias", [x], [y], initializer=[bias]
    )
    model = helper.make_model(
        graph,
        producer_name="melography-s3-spike",
        opset_imports=[helper.make_opsetid("", 13)],
        # IR 13: the newest IR (14, written by onnx>=1.23 by default) is
        # rejected by the ONNX Runtime bundled with ort 2.0.0-rc.15 (max
        # supported IR version: 13). Pin 13 so the artifact loads there.
        ir_version=13,
    )
    onnx.checker.check_model(model)
    return model


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument(
        "--out",
        default="spikes/s3-ort/models/dummy_identity.onnx",
        help="output path for the .onnx file",
    )
    args = ap.parse_args()
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    model = build_model()
    data = model.SerializeToString()
    out.write_bytes(data)
    sha = hashlib.sha256(data).hexdigest()
    print(f"wrote {out} ({len(data)} bytes, sha256={sha})")


if __name__ == "__main__":
    main()
