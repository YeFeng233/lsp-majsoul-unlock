#!/usr/bin/env python3
"""Download the current MajsoulData release and refresh local data artifacts."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import requests
from google.protobuf import descriptor_pb2


ROOT = Path(__file__).resolve().parents[1]
DATA_DIR = ROOT / "liqi_config"
SETTINGS_PATH = DATA_DIR / "settings.json"
RELEASE_URL = "https://api.github.com/repos/Avenshy/MajsoulData/releases/latest"
ASSET_NAMES = {"liqi.desc", "max_data.yaml"}


def headers(token: str) -> dict[str, str]:
    result = {"X-GitHub-Api-Version": "2022-11-28"}
    if token:
        result["Authorization"] = f"Bearer {token}"
    return result


def generate_liqi_json(descriptor: bytes) -> str:
    file_set = descriptor_pb2.FileDescriptorSet.FromString(descriptor)
    rpc_map = {}
    for file in file_set.file:
        for service in file.service:
            for method in service.method:
                name = f".{file.package}.{service.name}.{method.name}"
                rpc_map[name] = {"req": method.input_type, "resp": method.output_type}
    return json.dumps(rpc_map, separators=(",", ":"))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--token", default="")
    args = parser.parse_args()

    response = requests.get(RELEASE_URL, headers=headers(args.token), timeout=20)
    response.raise_for_status()
    release = response.json()
    assets = {
        asset["name"]: asset
        for asset in release.get("assets", [])
        if asset.get("name") in ASSET_NAMES
    }
    missing = ASSET_NAMES - assets.keys()
    if missing:
        raise RuntimeError(f"MajsoulData release is missing: {sorted(missing)}")

    DATA_DIR.mkdir(exist_ok=True)
    downloaded = {}
    for name, asset in sorted(assets.items()):
        item = requests.get(
            asset["browser_download_url"],
            headers=headers(args.token),
            timeout=20,
        )
        item.raise_for_status()
        downloaded[name] = item.content

    for name, content in downloaded.items():
        (DATA_DIR / name).write_bytes(content)
        print(f"updated {name}")

    (DATA_DIR / "liqi.json").write_text(
        generate_liqi_json(downloaded["liqi.desc"]), encoding="utf-8"
    )
    version = release.get("tag_name", "unknown")
    settings = json.loads(SETTINGS_PATH.read_text(encoding="utf-8"))
    settings["liqiVersion"] = version
    SETTINGS_PATH.write_text(json.dumps(settings, indent=2) + "\n", encoding="utf-8")
    print(f"updated liqi.json and settings.json ({version})")


if __name__ == "__main__":
    main()
