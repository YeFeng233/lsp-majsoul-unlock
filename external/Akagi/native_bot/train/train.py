#!/usr/bin/env python
"""Behavior-cloning trainer for the Akagi built-in bot.

Reads the Rust extractor's `(obs u8, action u16, mask u8)` shards, trains a small
1-D ResNet CNN with masked cross-entropy, then folds BatchNorm into the
preceding convolutions and exports a `.safetensors` file whose layout the Rust
`native_bot::model` (candle) loader reads directly.

Usage:
  python train.py --data out/p4 --out ../weights/akagi4p.safetensors \
      --epochs 8 --batch 1024 --lr 1e-3
"""
import argparse
import json
import os
import time

import numpy as np
import torch
import torch.nn as nn
import torch.nn.functional as F
from safetensors.torch import save_file

# Architecture constants — MUST match native_bot::model on the Rust side.
CONV = 64
BLOCKS = 3
FC = 256


# --------------------------------------------------------------------------- #
# Data
# --------------------------------------------------------------------------- #
def load_dataset(data_dir):
    meta = json.load(open(os.path.join(data_dir, "meta.json")))
    n = meta["total_samples"]
    obs_len = meta["obs_len"]
    A = meta["action_space"]
    counts = meta["shard_samples"]

    obs = np.empty((n, obs_len), dtype=np.uint8)
    act = np.empty((n,), dtype=np.int64)
    msk = np.empty((n, A), dtype=np.uint8)

    off = 0
    for i, c in enumerate(counts):
        if c == 0:
            continue
        o = np.fromfile(os.path.join(data_dir, f"{i}.obs"), dtype=np.uint8)
        a = np.fromfile(os.path.join(data_dir, f"{i}.act"), dtype="<u2")
        m = np.fromfile(os.path.join(data_dir, f"{i}.msk"), dtype=np.uint8)
        assert o.size == c * obs_len, f"shard {i} obs size mismatch"
        assert a.size == c and m.size == c * A, f"shard {i} act/msk size mismatch"
        obs[off:off + c] = o.reshape(c, obs_len)
        act[off:off + c] = a.astype(np.int64)
        msk[off:off + c] = m.reshape(c, A)
        off += c
    assert off == n, (off, n)
    return meta, obs, act, msk


# --------------------------------------------------------------------------- #
# Model
# --------------------------------------------------------------------------- #
class ResBlock(nn.Module):
    def __init__(self, ch):
        super().__init__()
        self.conv1 = nn.Conv1d(ch, ch, 3, padding=1, bias=False)
        self.bn1 = nn.BatchNorm1d(ch)
        self.conv2 = nn.Conv1d(ch, ch, 3, padding=1, bias=False)
        self.bn2 = nn.BatchNorm1d(ch)

    def forward(self, x):
        y = F.relu(self.bn1(self.conv1(x)))
        y = self.bn2(self.conv2(y))
        return F.relu(y + x)


class Net(nn.Module):
    def __init__(self, c_in, tile_dim, n_actions, conv=CONV, blocks=BLOCKS, fc=FC):
        super().__init__()
        self.conv_in = nn.Conv1d(c_in, conv, 3, padding=1, bias=False)
        self.bn_in = nn.BatchNorm1d(conv)
        self.res = nn.ModuleList([ResBlock(conv) for _ in range(blocks)])
        self.fc = nn.Linear(conv * tile_dim, fc)
        self.head = nn.Linear(fc, n_actions)

    def forward(self, x):
        x = F.relu(self.bn_in(self.conv_in(x)))
        for r in self.res:
            x = r(x)
        x = torch.flatten(x, 1)
        x = F.relu(self.fc(x))
        return self.head(x)


# --------------------------------------------------------------------------- #
# BatchNorm folding (Conv1d(bias=False) -> BN  ==>  Conv1d(bias=True))
# --------------------------------------------------------------------------- #
def fold(conv_w, bn):
    scale = bn.weight / torch.sqrt(bn.running_var + bn.eps)
    w = conv_w * scale.reshape(-1, 1, 1)
    b = bn.bias - bn.running_mean * scale
    return w.detach(), b.detach()


def folded_state_dict(model):
    sd = {}
    w, b = fold(model.conv_in.weight, model.bn_in)
    sd["conv_in.weight"], sd["conv_in.bias"] = w, b
    for i, r in enumerate(model.res):
        w1, b1 = fold(r.conv1.weight, r.bn1)
        w2, b2 = fold(r.conv2.weight, r.bn2)
        sd[f"res.{i}.conv1.weight"], sd[f"res.{i}.conv1.bias"] = w1, b1
        sd[f"res.{i}.conv2.weight"], sd[f"res.{i}.conv2.bias"] = w2, b2
    sd["fc.weight"], sd["fc.bias"] = model.fc.weight.detach(), model.fc.bias.detach()
    sd["head.weight"], sd["head.bias"] = model.head.weight.detach(), model.head.bias.detach()
    return {k: v.contiguous().cpu() for k, v in sd.items()}


def folded_forward(sd, x, tile_dim):
    """Pure conv+linear forward (no BN) — used to verify folding parity."""
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


# --------------------------------------------------------------------------- #
# Train
# --------------------------------------------------------------------------- #
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--epochs", type=int, default=8)
    ap.add_argument("--batch", type=int, default=1024)
    ap.add_argument("--lr", type=float, default=1e-3)
    ap.add_argument("--val-frac", type=float, default=0.02)
    ap.add_argument("--device", default="cuda" if torch.cuda.is_available() else "cpu")
    args = ap.parse_args()

    torch.manual_seed(0)
    np.random.seed(0)

    meta, obs, act, msk = load_dataset(args.data)
    C, T, A = meta["channels"], meta["tile_dim"], meta["action_space"]
    obs_scale = meta["obs_scale"]
    n = meta["total_samples"]
    print(f"loaded {n} samples  C={C} T={T} A={A}  device={args.device}")

    perm = np.random.permutation(n)
    n_val = int(n * args.val_frac)
    val_idx, train_idx = perm[:n_val], perm[n_val:]

    dev = torch.device(args.device)
    model = Net(C, T, A).to(dev)
    opt = torch.optim.AdamW(model.parameters(), lr=args.lr, weight_decay=1e-4)
    steps = args.epochs * (len(train_idx) // args.batch + 1)
    sched = torch.optim.lr_scheduler.CosineAnnealingLR(opt, T_max=steps, eta_min=args.lr * 0.02)

    obs_t = torch.from_numpy(obs)  # uint8, on CPU; slice+move per batch
    act_t = torch.from_numpy(act)
    msk_t = torch.from_numpy(msk)

    def batch(idx):
        xb = obs_t[idx].to(dev, non_blocking=True).float().div_(255.0).mul_(obs_scale)
        xb = xb.view(-1, C, T)
        ab = act_t[idx].to(dev, non_blocking=True)
        mb = msk_t[idx].to(dev, non_blocking=True).bool()
        return xb, ab, mb

    def evaluate():
        model.eval()
        correct = tot = 0
        vloss = 0.0
        with torch.no_grad():
            for s in range(0, len(val_idx), args.batch):
                idx = val_idx[s:s + args.batch]
                xb, ab, mb = batch(idx)
                logits = model(xb).masked_fill(~mb, -1e9)
                vloss += F.cross_entropy(logits, ab, reduction="sum").item()
                correct += (logits.argmax(1) == ab).sum().item()
                tot += len(idx)
        return vloss / max(tot, 1), correct / max(tot, 1)

    for ep in range(args.epochs):
        model.train()
        np.random.shuffle(train_idx)
        t0 = time.time()
        run = 0.0
        nb = 0
        for s in range(0, len(train_idx), args.batch):
            idx = train_idx[s:s + args.batch]
            xb, ab, mb = batch(idx)
            logits = model(xb).masked_fill(~mb, -1e9)
            loss = F.cross_entropy(logits, ab)
            opt.zero_grad(set_to_none=True)
            loss.backward()
            opt.step()
            sched.step()
            run += loss.item()
            nb += 1
        vloss, vacc = evaluate()
        print(f"epoch {ep+1}/{args.epochs}  train_loss={run/max(nb,1):.4f}  "
              f"val_loss={vloss:.4f}  val_top1={vacc*100:.2f}%  ({time.time()-t0:.1f}s)")

    # Fold BN and verify parity, then export. Folding is analytically exact;
    # float32 rounding through the deep net leaves a small absolute logit diff,
    # so we check that the *decisions* (argmax) still agree, which is what the
    # bot actually uses.
    model.eval()
    sd = folded_state_dict(model)
    with torch.no_grad():
        xb, _, mb = batch(val_idx[: min(4096, len(val_idx))])
        ref = model(xb)
        fol = folded_forward({k: v.to(dev) for k, v in sd.items()}, xb, T)
        max_diff = (ref - fol).abs().max().item()
        # Agreement over legal actions (matches inference masking).
        ref_a = ref.masked_fill(~mb, -1e9).argmax(1)
        fol_a = fol.masked_fill(~mb, -1e9).argmax(1)
        agree = (ref_a == fol_a).float().mean().item()
    print(f"BN-fold parity: max|diff|={max_diff:.2e}  argmax_agreement={agree*100:.2f}%")
    assert agree > 0.995, "BN folding changed too many decisions"

    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    save_file(
        sd,
        args.out,
        metadata={
            "num_players": str(meta["num_players"]),
            "channels": str(C),
            "tile_dim": str(T),
            "action_space": str(A),
            "conv": str(CONV),
            "blocks": str(BLOCKS),
            "fc": str(FC),
            "obs_scale": str(obs_scale),
        },
    )
    print(f"wrote {args.out}")


if __name__ == "__main__":
    main()
