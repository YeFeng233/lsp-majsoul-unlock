# Export an Akagi policy as a local ONNX model

The Android assistant accepts one self-contained policy file per player count, up to 128 MiB. The `akagi-policy-v1` contract requires one float32 observation input and one float32 policy output; tensor names are resolved from their shapes. Four-player shapes are `[1,39,34]` and `[1,82]`; three-player shapes are `[1,37,27]` and `[1,60]`. The model metadata must contain `majmax.contract=akagi-policy-v1`, `majmax.players=4` or `3`, `majmax.obs_schema=1`, and `majmax.action_codec=riichienv-core-0.4.8`. Features use Akagi native_bot's channel-major observation encoder and its matching action codec.

Install Python dependencies `torch`, `safetensors`, `onnx`, `onnxruntime`, and `numpy`. From the repository root, export either bundled model:

```powershell
python tools/export_policy_onnx.py --players 4 --output .\akagi4p.onnx
python tools/export_policy_onnx.py --players 3 --output .\akagi3p.onnx
```

The tool loads the matching safetensors weights, exports the folded Conv1d residual network with static tensor shapes, stamps the required metadata, then evaluates the same deterministic feature tensor through Python ONNX Runtime and the Rust/Candle model. It refuses to write the requested file if the maximum absolute logit difference is above `1e-3`.

Renaming a PyTorch checkpoint to `.onnx` does not convert it. This exporter only converts the bundled Akagi architecture. Starting in 0.10.0, the Android importer separately recognizes ZIP-based Mortal checkpoints by content, even if incorrectly named `.onnx`.

## Experimental Mortal v4 (four players only)

The separate experimental importer accepts ONNX `[1,1012,34] -> [1,46]`, optionally with one `[1,46]` legal-action mask of type BOOL or FLOAT. Tensor names may differ. Live inference receives the current legal-action mask from Rust, including red-five and claim action mapping. Official graphs may return negative infinity for illegal actions; only legal outputs must be finite and within the supported Q range. The app does not fabricate an all-legal mask during live inference.

Matching Mortal v4 PyTorch ZIP checkpoints (`.pth` / `.pt`) can be loaded locally by the Rust/Candle path, without Python conversion. The loader requires a recognized complete network layout, validates a forward pass, and rejects unsupported structures. Import and self-test validate without replacing an active engine's contract; activation happens when the assistant synchronizes model selection.

Three-player Mortal, old four-player versions, external ONNX weight files, arbitrary PyTorch modules, and oversized archives are rejected. This port still approximates parts of the upstream state feature tables and has not passed complete upstream observation/network parity. **Mortal recommendations are experimental and manual-only.** Enabling or selecting Mortal prevents unattended operation. Successful import is a format and inference check, not evidence of playing strength. No Mortal weights are shipped.

The Rust reference runner can also be invoked directly with `--weights`, `--players`, and `--input`, where `--input` is a JSON array containing the flattened channel-major observation. Android still performs its own metadata, node, shape, tensor type, operator, sample-inference, and finite-logit checks at import time.
