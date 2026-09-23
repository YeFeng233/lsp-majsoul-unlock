"""Check native ABI completeness, ELF architecture, dependencies and page alignment."""

import argparse
import pathlib
import struct
import zipfile


ABIS = {
    "arm64-v8a": (2, 183, 16384),
    "armeabi-v7a": (1, 40, 4096),
    "x86": (1, 3, 4096),
    "x86_64": (2, 62, 16384),
}
REQUIRED = {
    "libmajsoulprobe.so", "libmajsoulmodder.so", "libmajsoulai.so",
    "libmajsoulai_jni.so", "libonnxruntime.so", "libonnxruntime4j_jni.so",
}
# NDK system libraries are supplied by Android, never bundled in the APK.
SYSTEM = {
    "libc.so", "libm.so", "libdl.so", "liblog.so", "libandroid.so", "libz.so",
    "libEGL.so", "libGLESv2.so", "libGLESv3.so", "libjnigraphics.so",
    "libOpenSLES.so", "libvulkan.so", "libmediandk.so", "libaaudio.so",
}


def inspect_elf(data, abi):
    elf_class, machine, page = ABIS[abi]
    if data[:6] != b"\x7fELF" + bytes((elf_class, 1)):
        raise ValueError("incorrect ELF class or byte order")
    if struct.unpack_from("<H", data, 18)[0] != machine:
        raise ValueError("ELF machine does not match ABI directory")
    if elf_class == 2:
        phoff = struct.unpack_from("<Q", data, 32)[0]
        phsize, phcount = struct.unpack_from("<HH", data, 54)
        phformat = "<IIQQQQQQ"
    else:
        phoff = struct.unpack_from("<I", data, 28)[0]
        phsize, phcount = struct.unpack_from("<HH", data, 42)
        phformat = "<IIIIIIII"
    segments = []
    for index in range(phcount):
        values = struct.unpack_from(phformat, data, phoff + index * phsize)
        if elf_class == 2:
            kind, _, offset, address, _, size, _, align = values
        else:
            kind, offset, address, _, size, _, _, align = values
        segments.append((kind, offset, address, size, align))
    loads = [segment for segment in segments if segment[0] == 1]
    if not loads or any(align < page or (address - offset) % page
                        for _, offset, address, _, align in loads):
        raise ValueError(f"PT_LOAD segments are not aligned to {page} bytes")
    dynamic = next((segment for segment in segments if segment[0] == 2), None)
    needed = []
    string_table = None
    if dynamic:
        _, offset, _, size, _ = dynamic
        fmt = "<qQ" if elf_class == 2 else "<iI"
        for pos in range(offset, offset + size, struct.calcsize(fmt)):
            tag, value = struct.unpack_from(fmt, data, pos)
            if tag == 0:
                break
            if tag == 1:
                needed.append(value)
            elif tag == 5:
                string_table = value
    if not needed:
        return set()
    if string_table is None:
        raise ValueError("DT_NEEDED has no string table")
    table_offset = next((offset + string_table - address
                         for _, offset, address, size, _ in loads
                         if address <= string_table < address + size), None)
    if table_offset is None:
        raise ValueError("dynamic string table is outside PT_LOAD segments")
    return {data[table_offset + pos:data.index(b"\0", table_offset + pos)].decode("ascii")
            for pos in needed}


def verify(apk, abis):
    expected = set(abis)
    with zipfile.ZipFile(apk) as archive, open(apk, "rb") as raw:
        entries = {}
        for item in archive.infolist():
            parts = pathlib.PurePosixPath(item.filename).parts
            if len(parts) == 3 and parts[0] == "lib" and parts[2].endswith(".so"):
                libraries = entries.setdefault(parts[1], {})
                if parts[2] in libraries:
                    raise ValueError(f"duplicate library: {item.filename}")
                libraries[parts[2]] = item
        if set(entries) != expected:
            raise ValueError(f"APK ABI set {sorted(entries)} != expected {sorted(expected)}")
        all_names = set().union(*(set(libraries) for libraries in entries.values()))
        for abi in abis:
            libraries = entries[abi]
            missing = (REQUIRED | all_names) - libraries.keys()
            if missing:
                raise ValueError(f"{abi}: missing libraries {sorted(missing)}")
            for name, item in sorted(libraries.items()):
                if item.compress_type != zipfile.ZIP_STORED:
                    raise ValueError(f"{item.filename}: native library must be uncompressed")
                raw.seek(item.header_offset + 26)
                name_length, extra_length = struct.unpack("<HH", raw.read(4))
                data_offset = item.header_offset + 30 + name_length + extra_length
                if data_offset % ABIS[abi][2]:
                    raise ValueError(f"{item.filename}: ZIP data is not page aligned")
                try:
                    needed = inspect_elf(archive.read(item), abi)
                except (ValueError, struct.error) as error:
                    raise ValueError(f"{item.filename}: {error}") from error
                unresolved = needed - libraries.keys() - SYSTEM
                if unresolved:
                    raise ValueError(f"{item.filename}: missing dependencies {sorted(unresolved)}")
            print(f"{abi}: {len(libraries)} libraries; architecture, dependencies and alignment OK")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("apk", type=pathlib.Path)
    parser.add_argument("--abis", nargs="+", choices=ABIS, required=True)
    args = parser.parse_args()
    verify(args.apk, args.abis)
