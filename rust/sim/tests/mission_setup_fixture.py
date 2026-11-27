"""Prepare a hard-linked test copy for the client and server setup-failure tests.

Pass a prepared content root, a new destination and a relative mission manifest.
The optional fourth argument selects a compiled wrapper. The setup tests use
BFE_SETUP_TEST_CONTENT and the damage pressure test uses BFE_SCRIPT_DAMAGE_CONTENT.
Make the setup copy at <folder>/swbf2/.content: the client's Instant Action test
starts the battle from the content folder that holds it, as the menu does.
"""
import hashlib
import json
from pathlib import Path
import subprocess
import sys


def replace_json(path, value):
    temp = path.with_suffix(".new")
    temp.write_text(json.dumps(value, separators=(",", ":")), encoding="utf-8")
    temp.replace(path)


def prepare(source, dest, relative_manifest, wrapper=None):
    if dest.exists():
        raise FileExistsError(dest)
    subprocess.run(["cp", "-al", "--", source, dest], check=True)
    manifest_file = dest / relative_manifest
    manifest = json.loads(manifest_file.read_bytes())
    map_file = manifest_file.parent.parent / "map.json"
    world = json.loads(map_file.read_bytes())
    body = (wrapper or Path(__file__).parent / "lua" / "21_startup.luac").read_bytes()
    digest = hashlib.sha256(body).hexdigest()
    stored = f"scripts/{digest}.luac"
    with (dest / stored).open("xb") as output:
        output.write(body)
    entry = manifest["scripts"][0]
    wrapper = dict(entry, file=stored, length=len(body), sha256=digest)
    manifest["scripts"][:1] = [wrapper, dict(entry, name="previous_setup", id="previous_setup")]
    manifest["chunks"].insert(1, "previous_setup")
    world["missions"][manifest["entry"]]["scripts"] = manifest["scripts"]
    replace_json(map_file, world)
    replace_json(manifest_file, manifest)


if __name__ == "__main__":
    prepare(*(Path(arg) for arg in sys.argv[1:]))
