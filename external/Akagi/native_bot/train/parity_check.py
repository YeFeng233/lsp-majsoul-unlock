#!/usr/bin/env python
"""Dump a deterministic input + reference logits for a folded safetensors model,
so the Rust candle loader can be checked for numeric parity.

Usage: parity_check.py <model.safetensors> <C> <T> <out.json>
"""
import json
import sys

import torch
import torch.nn.functional as F
from safetensors.torch import load_file


def folded_forward(sd, x):
    x = F.relu(F.conv1d(x, sd["conv_in.weight"], sd["conv_in.bias"], padding=1))
    i = 0
    while f"res.{i}.conv1.weight" in sd:
        y = F.relu(F.conv1d(x, sd[f"res.{i}.conv1.weight"], sd[f"res.{i}.conv1.bias"], padding=1))
        y = F.conv1d(y, sd[f"res.{i}.conv2.weight"], sd[f"res.{i}.conv2.bias"], padding=1)
        x = F.relu(y + x)
        i += 1
    x = torch.flatten(x, 1)
    x = F.relu(F.linear(x, sd["fc.weight"], sd["fc.bias"]))
    return F.linear(x, sd["head.weight"], sd["head.bias"])


def main():
    path, C, T, out = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
    sd = load_file(path)
    n = C * T
    # Deterministic input in [0, 4], same values the Rust test will read back.
    inp = [((i * 7 + 3) % 13) / 13.0 * 4.0 for i in range(n)]
    x = torch.tensor(inp, dtype=torch.float32).view(1, C, T)
    with torch.no_grad():
        logits = folded_forward(sd, x).view(-1).tolist()
    json.dump(
        {"C": C, "T": T, "input": inp, "logits": logits, "argmax": int(max(range(len(logits)), key=lambda i: logits[i]))},
        open(out, "w"),
    )
    print(f"{path}: wrote {out}  argmax={max(range(len(logits)), key=lambda i: logits[i])}")


if __name__ == "__main__":
    main()
