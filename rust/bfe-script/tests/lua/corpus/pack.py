#!/usr/bin/env python3
"""Compile every cNNN.lua next to this script with a reference Lua 5.0.3 `luac` built
with the 2005 game's chunk settings (see ../README.md), run it in the matching
reference `lua` under LC_ALL=C, and write corpus.bin.

Usage: pack.py LUAC LUA           write corpus.bin
       pack.py --check LUAC LUA   rebuild it in memory and compare with corpus.bin,
                                  byte for byte; exit status 1 on any change

corpus.bin layout (little-endian): b"BFE2", u32 count, then per program u32 name
length, u32 chunk length, u32 output length, u64 FNV-1a hash of the source file,
the name, the compiled chunk and the reference stdout. Programs are compiled from
this folder under their bare file names, so chunk names carry no path. The test
checks every committed source against its recorded hash, so a source edited without
repacking fails it.
"""
import glob
import os
import struct
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))


def fnv1a64(data):
    h = 0xCBF29CE484222325
    for b in data:
        h = ((h ^ b) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def build(luac, lua):
    env = dict(os.environ, LC_ALL="C")
    entries = []
    with tempfile.TemporaryDirectory() as tmp:
        for src in sorted(glob.glob(os.path.join(HERE, "c*.lua"))):
            name = os.path.basename(src)[:-4]
            out = os.path.join(tmp, name + ".luac")
            subprocess.run([luac, "-o", out, name + ".lua"], cwd=HERE, check=True)
            run = subprocess.run([lua, out], cwd=tmp, env=env, stdout=subprocess.PIPE, check=True)
            with open(out, "rb") as f:
                chunk = f.read()
            with open(src, "rb") as f:
                source_hash = fnv1a64(f.read())
            entries.append((name.encode(), chunk, run.stdout, source_hash))
    blob = bytearray(b"BFE2" + struct.pack("<I", len(entries)))
    for name, chunk, stdout, source_hash in entries:
        blob += struct.pack("<IIIQ", len(name), len(chunk), len(stdout), source_hash)
        blob += name + chunk + stdout
    return bytes(blob), len(entries)


def main():
    args = sys.argv[1:]
    check = bool(args) and args[0] == "--check"
    if check:
        args = args[1:]
    luac, lua = (os.path.abspath(p) for p in args[:2])
    blob, count = build(luac, lua)
    path = os.path.join(HERE, "corpus.bin")
    if check:
        with open(path, "rb") as f:
            if f.read() != blob:
                print("corpus.bin differs from a rebuild of the %d committed programs" % count)
                sys.exit(1)
        print("corpus.bin matches a rebuild of the %d committed programs" % count)
        return
    with open(path, "wb") as f:
        f.write(blob)
    print("packed %d programs" % count)


if __name__ == "__main__":
    main()
