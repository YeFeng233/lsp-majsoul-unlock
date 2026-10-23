"""Make an inference-only Mortal v4 checkpoint without changing any model weights.

Requires a current CPU PyTorch. Uses weights_only=True; never runs checkpoint code.
The Android app still performs its complete architecture and inference validation.
"""
import argparse
import hashlib
from pathlib import Path
import zipfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    if args.input.resolve() == args.output.resolve() or args.output.exists():
        parser.error("Choose a new output file; the source and existing files are never overwritten")
    if not 0 < args.input.stat().st_size <= 300 * 1024 * 1024:
        parser.error("Input exceeds the Android 300 MiB limit")
    with zipfile.ZipFile(args.input) as archive:
        if sum(entry.file_size for entry in archive.infolist()) > 600 * 1024 * 1024:
            parser.error("Checkpoint exceeds the Android 600 MiB uncompressed limit")
    import torch
    source = torch.load(args.input, map_location="cpu", weights_only=True)
    brain, head = source["mortal"], source["current_dqn"]
    entry = brain["encoder.net.0.weight"]
    width, channels, kernel = entry.shape
    actions = {775: 44, 1012: 46}.get(channels)
    if actions is None or kernel != 3 or width > 512:
        parser.error("Only Mortal v4 four-player 1012/46 and sanma 775/44 layouts are supported")
    if tuple(head["net.weight"].shape) != (actions + 1, 1024) or tuple(head["net.bias"].shape) != (actions + 1,):
        parser.error("The checkpoint does not contain the matching v4 dueling head")
    config = source.get("config", {})
    if config.get("control", {}).get("version", 4) != 4:
        parser.error("Checkpoint declares an unsupported model version")
    blocks = 0
    while f"encoder.net.{blocks+1}.res_unit.2.weight" in brain:
        blocks += 1
    if not 1 <= blocks <= 128:
        parser.error("No supported residual-block sequence found")
    networks = {}
    for name, state in (("mortal", brain), ("current_dqn", head)):
        networks[name] = {}
        for key, value in state.items():
            if not isinstance(value, torch.Tensor):
                parser.error(f"Unexpected non-tensor in {name}.{key}")
            if value.is_floating_point() and not torch.isfinite(value).all():
                parser.error(f"Non-finite model weight: {name}.{key}")
            networks[name][key] = value.detach().cpu().contiguous()
    networks["config"] = {"control": {"version": 4}, "resnet": {"conv_channels": width, "num_blocks": blocks}}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    temporary = args.output.with_name(args.output.name + ".tmp")
    if temporary.exists():
        parser.error("Temporary output already exists")
    try:
        torch.save(networks, temporary)
        checked = torch.load(temporary, map_location="cpu", weights_only=True)
        for root in ("mortal", "current_dqn"):
            assert checked[root].keys() == networks[root].keys()
            for key, value in networks[root].items():
                assert torch.equal(value, checked[root][key]), f"Weight changed: {root}.{key}"
        temporary.rename(args.output)
    finally:
        temporary.unlink(missing_ok=True)
    with args.output.open("rb") as file:
        digest = hashlib.file_digest(file, "sha256").hexdigest()
    print(f"{3 if channels == 775 else 4}-player Mortal v4; {blocks} blocks; {actions} actions")
    print(f"All {len(brain)+len(head)} inference tensors preserved exactly")
    print(f"{args.input.stat().st_size} -> {args.output.stat().st_size} bytes")
    print(f"Output: {args.output.resolve()}\nSHA-256: {digest}")


if __name__ == "__main__":
    main()
