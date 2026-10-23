"""Publish the existing unlock IDs as a versioned, checksummed JSON catalog."""
import argparse
import hashlib
import json
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "external/MajsoulMax-rs/liqi_config/max_data.yaml"
DEST = ROOT / "catalog"
KEYS = ("character", "skin", "title", "item", "loading_image", "emoji", "endings")


def source_data():
    data, section, character = {}, None, None
    for line in SOURCE.read_text(encoding="utf8").splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        if not line.startswith(" ") and line.endswith(":"):
            section = line[:-1]
            if section not in KEYS or section in data:
                raise ValueError("Invalid or duplicate section")
            data[section] = {} if section == "emoji" else []
        elif section == "emoji" and line.strip().endswith(":"):
            character = str(int(line.strip()[:-1]))
            if character in data[section]:
                raise ValueError("Duplicate emoji character")
            data[section][character] = []
        else:
            if not line.strip().startswith("- "):
                raise ValueError("Invalid ID line")
            value = int(line.strip()[2:])
            if not 0 < value <= 0xffffffff:
                raise ValueError("ID outside unsigned 32-bit range")
            target = data[section][character] if section == "emoji" else data[section]
            if value in target:
                raise ValueError("Duplicate ID")
            target.append(value)
    if set(data) != set(KEYS) or not all(data.values()):
        raise ValueError("Incomplete source catalog")
    return data


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", type=int)
    parser.add_argument("--name")
    parser.add_argument("--date")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    manifest_path = DEST / "manifest.json"
    data_path = DEST / "unlock-data.json"
    expected = source_data()
    if args.check:
        manifest = json.loads(manifest_path.read_text(encoding="utf8"))
        payload = data_path.read_bytes()
        assert json.loads(payload) == expected, "Catalog and bundled YAML differ; regenerate catalog"
        assert manifest["schemaVersion"] == 1 and manifest["version"] > 0
        assert manifest["dataFile"] == data_path.name and manifest["minModuleVersionCode"] == 25
        assert manifest["size"] == len(payload) <= 512 * 1024
        assert manifest["sha256"] == hashlib.sha256(payload).hexdigest()
        print("Unlock catalog, checksum and bundled YAML agree.")
        return
    if not args.version or args.version < 0 or args.version > 9_007_199_254_740_991 or not args.name or not args.date:
        parser.error("--version, --name and --date are required when publishing")
    date.fromisoformat(args.date)
    if len(args.name) > 64 or any(ord(char) < 32 for char in args.name):
        parser.error("Invalid version name")
    if manifest_path.exists():
        old = json.loads(manifest_path.read_text(encoding="utf8"))
        if args.version <= old["version"]:
            parser.error("Published catalog versions must increase")
    DEST.mkdir(exist_ok=True)
    payload = (json.dumps(expected, ensure_ascii=False, indent=2) + "\n").encode("utf8")
    data_path.write_bytes(payload)
    manifest = {
        "schemaVersion": 1, "version": args.version, "versionName": args.name,
        "updatedAt": args.date, "minModuleVersionCode": 25,
        "dataFile": data_path.name, "size": len(payload),
        "sha256": hashlib.sha256(payload).hexdigest(),
    }
    manifest_path.write_bytes((json.dumps(manifest, indent=2) + "\n").encode("utf8"))
    print("Published", args.name, "with", len(expected["skin"]), "skins")


if __name__ == "__main__":
    main()
