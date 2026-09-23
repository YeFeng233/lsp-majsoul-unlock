# Export an Akagi policy as a local ONNX model

The Android assistant accepts one self-contained ONNX file per player count. Its contract is documented in [FEATURE_UPGRADE_DESIGN.md](../docs/FEATURE_UPGRADE_DESIGN.md): input `obs`, output `logits`, fixed four-player or three-player dimensions, and the `akagi-policy-v1` metadata.

Install Python dependencies `torch`, `safetensors`, `onnx`, `onnxruntime`, and `numpy`. From the repository root, export either bundled model:

```powershell
python tools/export_policy_onnx.py --players 4 --output .\akagi4p.onnx
python tools/export_policy_onnx.py --players 3 --output .\akagi3p.onnx
```

The tool loads the matching safetensors weights, exports the folded Conv1d residual network with static tensor shapes, stamps the required metadata, then evaluates the same deterministic feature tensor through Python ONNX Runtime and the Rust/Candle model. It refuses to write the requested file if the maximum absolute logit difference is above `1e-3`.

The Rust reference runner can also be invoked directly with `--weights`, `--players`, and `--input`, where `--input` is a JSON array containing the flattened channel-major observation. Android still performs its own metadata, node, shape, tensor type, operator, sample-inference, and finite-logit checks at import time.
