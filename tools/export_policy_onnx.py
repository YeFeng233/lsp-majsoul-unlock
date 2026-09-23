#!/usr/bin/env python3
"""Export folded Akagi safetensors weights to the MajsoulMax ONNX policy contract.

Requires torch, safetensors, onnx, onnxruntime and numpy. The script compares
the exported model against the Rust/Candle implementation with a deterministic
feature tensor and refuses to emit a model outside the 1e-3 logit tolerance.
"""
import argparse
import json
import pathlib
import subprocess
import tempfile

import numpy as np
import onnx
import onnxruntime as ort
import torch
import torch.nn as nn
import torch.nn.functional as F
from safetensors.torch import load_file

CONV = 64
BLOCKS = 3
FC = 256


class Residual(nn.Module):
    def __init__(self):
        super().__init__()
        self.conv1 = nn.Conv1d(CONV, CONV, 3, padding=1)
        self.conv2 = nn.Conv1d(CONV, CONV, 3, padding=1)

    def forward(self, value):
        return F.relu(self.conv2(F.relu(self.conv1(value))) + value)


class Policy(nn.Module):
    def __init__(self, channels, tiles, actions):
        super().__init__()
        self.conv_in = nn.Conv1d(channels, CONV, 3, padding=1)
        self.res = nn.ModuleList([Residual() for _ in range(BLOCKS)])
        self.fc = nn.Linear(CONV * tiles, FC)
        self.head = nn.Linear(FC, actions)

    def forward(self, obs):
        value = F.relu(self.conv_in(obs))
        for block in self.res:
            value = block(value)
        value = torch.flatten(value, 1)
        return self.head(F.relu(self.fc(value)))


def main():
    root = pathlib.Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser()
    parser.add_argument("--players", type=int, choices=(3, 4), required=True)
    parser.add_argument("--weights", type=pathlib.Path)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--tolerance", type=float, default=1e-3)
    args = parser.parse_args()
    tiles = 34 if args.players == 4 else 27
    channels = 39 if args.players == 4 else 37
    actions = 82 if args.players == 4 else 60
    weights = args.weights or root / "external" / "Akagi" / "native_bot" / "weights" / f"akagi{args.players}p.safetensors"
    weights = weights.resolve()

    torch.manual_seed(20260923)
    model = Policy(channels, tiles, actions).eval()
    model.load_state_dict(load_file(str(weights)), strict=True)
    obs = np.random.default_rng(20260923).integers(0, 2, size=(1, channels, tiles), dtype=np.uint8).astype(np.float32)
    input_tensor = torch.from_numpy(obs)
    args.output.parent.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="majmax-onnx-") as temp_dir:
        temp = pathlib.Path(temp_dir)
        model_path = temp / "candidate.onnx"
        input_path = temp / "obs.json"
        input_path.write_text(json.dumps(obs.reshape(-1).tolist()), encoding="utf-8")
        torch.onnx.export(model, input_tensor, str(model_path), input_names=["obs"],
                          output_names=["logits"], opset_version=17, do_constant_folding=True,
                          dynamic_axes=None, dynamo=False)

        proto = onnx.load(str(model_path), load_external_data=False)
        del proto.metadata_props[:]
        metadata = {
            "majmax.contract": "akagi-policy-v1",
            "majmax.players": str(args.players),
            "majmax.obs_schema": "1",
            "majmax.action_codec": "riichienv-core-0.4.8",
        }
        for key, value in metadata.items():
            item = proto.metadata_props.add()
            item.key, item.value = key, value
        proto.doc_string = f"Akagi local policy for {args.players} players"
        onnx.save(proto, str(model_path))

        session = ort.InferenceSession(str(model_path), providers=["CPUExecutionProvider"])
        exported = session.run(["logits"], {"obs": obs})[0].reshape(-1)
        rust_command = [
            "cargo", "run", "--release", "--quiet", "--locked",
            "--manifest-path", str(root / "hook-probe" / "rust-ai" / "Cargo.toml"),
            "--bin", "policy_logits", "--", "--weights", str(weights),
            "--players", str(args.players), "--input", str(input_path),
        ]
        reference = np.asarray(json.loads(subprocess.check_output(rust_command, cwd=root, text=True)), dtype=np.float32)
        max_error = float(np.max(np.abs(exported - reference)))
        if max_error > args.tolerance:
            raise SystemExit(f"Candle/ONNX logits differ by {max_error:.6g} (limit {args.tolerance})")
        args.output.write_bytes(model_path.read_bytes())
        print(f"wrote {args.output} · players={args.players} · max|Δlogits|={max_error:.6g}")


if __name__ == "__main__":
    main()
