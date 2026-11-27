"""Compile the execution-record programs with a float, four-byte-length Lua 5.0.3 luac."""

import argparse
import gzip
import json
import subprocess
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("luac", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    compiler = args.luac.resolve(strict=True)
    here = Path(__file__).resolve().parent
    with gzip.open(here / "sources.jsonl.gz", "rt", encoding="utf-8") as f:
        sources = dict(map(json.loads, f))
    for path in (here.parent / "corpus").glob("c*.lua"):
        sources[f"corpus/{path.stem}"] = path.read_bytes().decode("utf-8")
    with gzip.open(here / "current.jsonl.gz", "rt", encoding="utf-8") as f:
        expected = [json.loads(row)[0] for row in f]
    if sorted(sources) != expected or len(expected) != 2360:
        parser.error("source names differ from the execution records")
    for name in expected:
        path = (args.output / name).with_suffix(".lua")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(sources[name].encode("utf-8"))
        # Only the basename belongs in Lua's serialized source name and errors.
        subprocess.run(
            [compiler, "-o", path.with_suffix(".luac").name, path.name],
            cwd=path.parent,
            check=True,
        )
    print(f"Compiled {len(expected)} programs")


if __name__ == "__main__":
    main()
