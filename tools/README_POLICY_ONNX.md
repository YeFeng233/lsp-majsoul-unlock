# Export an Akagi policy as a local ONNX model

The Android assistant accepts one self-contained policy file per player count, up to 300 MiB. PyTorch ZIP checkpoints additionally have a 600 MiB total uncompressed limit, a 300 MiB per-entry limit and a 4 MiB metadata limit. The `akagi-policy-v1` contract requires one float32 observation input and one float32 policy output; tensor names are resolved from their shapes. Four-player shapes are `[1,39,34]` and `[1,82]`; three-player shapes are `[1,37,27]` and `[1,60]`. The model metadata must contain `majmax.contract=akagi-policy-v1`, `majmax.players=4` or `3`, `majmax.obs_schema=1`, and `majmax.action_codec=riichienv-core-0.4.8`. Features use Akagi native_bot's channel-major observation encoder and its matching action codec.

Install Python dependencies `torch`, `safetensors`, `onnx`, `onnxruntime`, and `numpy`. From the repository root, export either bundled model:

```powershell
python tools/export_policy_onnx.py --players 4 --output .\akagi4p.onnx
python tools/export_policy_onnx.py --players 3 --output .\akagi3p.onnx
```

The tool loads the matching safetensors weights, exports the folded Conv1d residual network with static tensor shapes, stamps the required metadata, then evaluates the same deterministic feature tensor through Python ONNX Runtime and the Rust/Candle model. It refuses to write the requested file if the maximum absolute logit difference is above `1e-3`.

Renaming a PyTorch checkpoint to `.onnx` does not convert it. This exporter only converts the bundled Akagi architecture. Starting in 0.10.0, the Android importer separately recognizes ZIP-based Mortal checkpoints by content, even if incorrectly named `.onnx`.

## Experimental Mortal v4 (four players and sanma)

The separate experimental importer accepts four-player ONNX `[1,1012,34] -> [1,46]` and Akagi-compatible sanma ONNX `[1,775,34] -> [1,44]`, optionally with one matching `[1,46]` or `[1,44]` legal-action mask of type BOOL or FLOAT. Tensor names may differ. The player count is detected from the complete input/output pair; crossed pairs are rejected. Live inference receives the current legal-action mask from Rust, including red-five and claim action mapping. The 775-channel sanma protocol uses pon=38, kan=39, kita=40; it differs from other sanma forks. Official graphs may return negative infinity for illegal actions; only legal outputs must be finite and within the supported Q range. The app does not fabricate an all-legal mask during live inference.

Matching Mortal v4 PyTorch ZIP checkpoints (`.pth` / `.pt`) can be loaded locally by the Rust/Candle path, without Python conversion. The loader requires a recognized complete network layout, validates a forward pass, and rejects unsupported structures. Import and self-test validate without replacing an active engine's contract; activation happens when the assistant synchronizes model selection.

Sanma support is available starting in 0.11.0. The importer stores each player count separately and preserves the other model selection. Old versions, external ONNX weight files, arbitrary PyTorch modules, and oversized archives are rejected. This port still approximates parts of the upstream state feature tables and has not passed complete upstream observation parity. **Mortal recommendations remain experimental.** Compatible validated custom models can run unattended when the user explicitly enables that mode; importing a model does not enable it. Model fallback, incomplete synchronization, stale decisions, exact hand/round checks and manual-input stop remain enforced. Successful import is a format and inference check, not evidence of playing strength. No Mortal weights are shipped.

Training checkpoints may include optimizer state that is unnecessary for inference. To create a smaller CPU checkpoint with all inference tensors unchanged, install CPU PyTorch and run:

```powershell
python tools/compact_mortal_checkpoint.py .\mortal.pth .\mortal-sanma-inference.pth
```

This uses `torch.load(weights_only=True)`, verifies an exact tensor round trip, and refuses to overwrite an existing output or the original file. It preserves the model's player count and architecture; it does not convert a sanma model into a four-player model. The Android importer still validates the complete network and runs a forward pass.

For local numerical and engine checks with your own checkpoint:

```powershell
cargo run --release --manifest-path external/Akagi/native_bot/Cargo.toml --example mortal_reference -- .\mortal.pth .\observation.json .\logits.json
cargo run --release --manifest-path external/Akagi/native_bot/Cargo.toml --example mortal_sanma_smoke -- .\mortal.pth
```

The Akagi reference runner (`policy_reference`) uses `--weights`, `--players`, and `--input`. Both reference runners take a JSON array containing the flattened channel-major observation. Android independently validates each imported file's contract and sample inference.
