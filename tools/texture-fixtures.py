#!/usr/bin/env python3
"""Regenerate synthetic renderer KTX2 inputs with KTX-Software 4.4.2.

The fixtures contain authored test patterns, never imported game content.
Run with --encoder pointing to the pinned ktx executable when it is not on PATH.
"""
import argparse
import struct
import subprocess
import tempfile
import zlib
from pathlib import Path


def chunk(kind, data):
    # Emit PNG framing ourselves so image-library versions cannot change the
    # source bytes or silently apply a colour profile to the authored channels.
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))


def png(alpha, width=16, height=8):
    # Distinct channels and partial alpha expose swizzles and premultiplication.
    # Raw, unprofiled bytes keep codec expectations independent of PNG viewers.
    rows = bytearray()
    for y in range(height):
        # Filter zero makes the raw row pattern directly inspectable. The
        # non-square dimensions expose accidental width/height interchange.
        rows.append(0)
        for x in range(width):
            rows.extend(((x * 17) & 255, (y * 31) & 255, ((x + y) * 23) & 255,
                         ((x * 13 + y * 29) & 255) if alpha else 255))
    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--encoder", default="ktx")
    args = parser.parse_args()
    version = subprocess.check_output([args.encoder, "--version"], text=True)
    # Encoded block goldens depend on the encoder version, unlike decoded PNG
    # rows. Require the pinned executable before replacing committed fixtures.
    if "4.4.2" not in version:
        raise SystemExit("Fixtures require KTX-Software 4.4.2")
    output = Path(__file__).resolve().parents[1] / "tests" / "fixtures" / "renderer-textures"
    output.mkdir(parents=True, exist_ok=True)
    # Keep encoder input filenames independent of the checked-out folder. The
    # same PNG bytes also exercise the Rust worker's PNG entry point directly.
    with tempfile.TemporaryDirectory() as temporary:
        for name, alpha, width, height, encoding in (
            ("opaque", False, 16, 8, "uastc"), ("alpha", True, 16, 8, "uastc"),
            ("opaque-etc1s", False, 13, 7, "basis-lz"),
            ("alpha-etc1s", True, 13, 7, "basis-lz"),
            ("alpha-odd", True, 13, 7, "uastc")):

            source = Path(temporary) / f"{name}.png"
            source.write_bytes(png(alpha,width,height))
            (output / f"{name}.png").write_bytes(source.read_bytes())
            # Generate every mip, including levels smaller than a block. Both
            # alpha classes run through the same UASTC/Zstandard container path;
            # capability selection is tested later by the shipped Rust decoder.
            command=[args.encoder, "create", "--format", "R8G8B8A8_UNORM", "--assign-tf", "linear",
                     "--generate-mipmap", "--mipmap-filter", "box", "--encode", encoding]
            if encoding=="uastc":command += ["--uastc-quality", "2", "--zstd", "18"]
            else:command += ["--qlevel", "128", "--clevel", "2"]
            subprocess.run(command+[str(source),str(output/f"{name}.ktx2")],check=True)
    # Goldens come from the pinned former decoder, independent of Rust's code.
    # Keeping the command here makes a fixture/hash update reproducible together.
    subprocess.run(["node",str(output.parents[2]/"tools"/"texture-fixture-goldens.mjs")],check=True)



if __name__ == "__main__":
    main()
