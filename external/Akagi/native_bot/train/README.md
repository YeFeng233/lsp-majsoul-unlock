# Training the built-in bot

The built-in bot is trained by **behavior cloning** (supervised imitation) of
human Tenhou logs. The pipeline is: extract `(obs, action, mask)` samples in
Rust → train a small CNN in Python on GPU → export `.safetensors` that the Rust
`native_bot::model` (candle) loads directly.

Everything except the ~minute of GPU training is pure Rust. Python is used
**only** offline to fit weights.

## 1. Extract training data (Rust)

The extractor reads mjai `.json.gz` game logs, replays each through
riichienv-core, and writes compact binary shards. Observations are stored as
`u8` (`value/4*255`, `obs_scale = 4.0`); actions as `u16`; masks as `u8`.

```sh
cd native_bot
cargo build --release --no-default-features --features extract --bin extract

# 4-player: first 40k games, capped at 6M samples
./target/release/extract  <dataset>/p4  4  out/p4  40000  6000000
# 3-player: capped at 4M samples
./target/release/extract  <dataset>/p3  3  out/p3  40000  4000000
```

Each output dir gets `N.obs` / `N.act` / `N.msk` shard files plus a `meta.json`
describing the geometry and sample counts. Extraction is multi-threaded (rayon)
and fast — millions of samples in seconds.

Notes:
- One file = one game; malformed logs are skipped (guarded by `catch_unwind`).
- Tenhou **sanma** logs use a 4-seat layout with a dummy 4th player and spell
  nukidora as `nukidora`. Both are handled by `mjai_compat::parse_line`: the
  rename runs *before* serde (riichienv's `MjaiEvent` has a `#[serde(other)]`
  catch-all, so `nukidora` would otherwise deserialize to `Other` and be
  dropped), the 4-seat arrays are truncated after.
- Discards dominate (~77–81%), with `pass` synthesized for every seat that
  could have called a discard but didn't. Sanma additionally spends ~6% of its
  samples on kita.
- Shards land in `native_bot/out/` (gitignored) — tens of GB. Only the trained
  `weights/*.safetensors` are committed.

## 2. Train (Python, GPU)

Create/activate a venv with `torch` (CUDA), `numpy`, `safetensors`.

```sh
python train.py --data out/p4 --out ../weights/akagi4p.safetensors \
    --epochs 10 --batch 2048 --lr 1.5e-3
python train.py --data out/p3 --out ../weights/akagi3p.safetensors \
    --epochs 10 --batch 2048 --lr 1.5e-3
```

The model is a 1-D ResNet over the tile axis (`conv 64`, `3` residual blocks,
`fc 256`) with a masked cross-entropy loss over legal actions. After training,
BatchNorm is **folded into the preceding convolutions** so the exported model is
pure Conv1d + Linear — the candle side needs no BatchNorm op. A parity check
asserts the fold preserves the argmax decisions.

Reference numbers for exactly the commands above (40k games → 6.0M/4.0M samples,
10 epochs, RTX 4070 Ti) — these are what the committed weights were trained with:

| | val top-1 | params | weights | train time |
|---|---|---|---|---|
| 4p | 75.9% | 660,050 | 2.6 MB | ~5 min |
| 3p | 77.8% | 539,324 | 2.2 MB | ~3 min |

## 3. Verify parity (Rust ⇄ Python)

`parity_check.py` dumps a deterministic input + reference logits so the Rust
loader can be checked numerically:

```sh
python parity_check.py ../weights/akagi4p.safetensors 39 34 ../weights/parity_4p.json
python parity_check.py ../weights/akagi3p.safetensors 37 27 ../weights/parity_3p.json
cd .. && cargo test --test parity     # candle must match PyTorch (max|diff| < 2e-3)
```

The exported `weights/*.safetensors` are `include_bytes!`-embedded by
`src/defaults.rs`, so rebuilding `native_bot` after retraining ships the new
weights automatically.

## Keeping the two sides in sync

`CONV` / `BLOCKS` / `FC` in `train.py` **must** equal the constants in
`src/model.rs`, and the safetensors key names must line up (`conv_in`,
`res.{i}.conv{1,2}`, `fc`, `head`). If you change the observation layout in
`src/obs.rs`, re-extract and re-train — weights are layout-specific.
