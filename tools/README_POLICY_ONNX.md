# Export an Akagi policy as a local ONNX model

The Android assistant accepts one self-contained ONNX file per player count. The `akagi-policy-v1` contract requires a float32 input named `obs` and a float32 output named `logits`. Four-player shapes are `[1,39,34]` and `[1,82]`; three-player shapes are `[1,37,27]` and `[1,60]`. The model metadata must contain `majmax.contract=akagi-policy-v1`, `majmax.players=4` or `3`, `majmax.obs_schema=1`, and `majmax.action_codec=riichienv-core-0.4.8`. Features use Akagi native_bot's channel-major observation encoder and its matching action codec.

Install Python dependencies `torch`, `safetensors`, `onnx`, `onnxruntime`, and `numpy`. From the repository root, export either bundled model:

```powershell
python tools/export_policy_onnx.py --players 4 --output .\akagi4p.onnx
python tools/export_policy_onnx.py --players 3 --output .\akagi3p.onnx
```

The tool loads the matching safetensors weights, exports the folded Conv1d residual network with static tensor shapes, stamps the required metadata, then evaluates the same deterministic feature tensor through Python ONNX Runtime and the Rust/Candle model. It refuses to write the requested file if the maximum absolute logit difference is above `1e-3`.

Renaming a PyTorch checkpoint to `.onnx` does not convert it. ZIP-based Mortal checkpoints containing `data.pkl` and `data/*` are not ONNX protobuf files, and this exporter does not convert Mortal weights: Mortal's network and feature/action contract differ from the bundled Akagi policy. The Android importer detects ZIP headers and reports this format mismatch before copying the archive.

The Rust reference runner can also be invoked directly with `--weights`, `--players`, and `--input`, where `--input` is a JSON array containing the flattened channel-major observation. Android still performs its own metadata, node, shape, tensor type, operator, sample-inference, and finite-logit checks at import time.
