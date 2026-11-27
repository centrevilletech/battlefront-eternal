"""Compiled script chains observed during a 2005 mission's setup."""

from dataclasses import dataclass
import hashlib
import json
from pathlib import Path
import re
import tempfile

from ucfb import child, chunks, node_name
from zero_family import SWBF2, profile


_ASCII_CASE = str.maketrans("ABCDEFGHIJKLMNOPQRSTUVWXYZ", "abcdefghijklmnopqrstuvwxyz")


def script_index(data):
    """Index the first scr_ for each ASCII-insensitive name in archive order."""
    found = {}
    stack = list(reversed(chunks(data, 0, len(data))))
    while stack:
        tag, start, end = stack.pop()
        if tag in (b"ucfb", b"lvl_"):
            children = chunks(data, start + (8 if tag == b"lvl_" else 0), end)
            stack.extend(reversed(children))
        elif tag == b"scr_":
            name = node_name(data, start, end)
            body = child(data, start, end, b"BODY")
            if name:
                key = name.translate(_ASCII_CASE)
                if key not in found:
                    found[key] = data[body[0]:body[1]] if body else None
    return found


def _requests(manifest):
    entry = manifest["script"]
    includes = []
    for call in manifest["calls"]:
        if call.get("name") == "ScriptCB_DoFile":
            args = call["args"]
            if not args or not isinstance(args[0], str):
                raise ValueError(f"{entry}: invalid ScriptCB_DoFile request {args!r}")
            includes.append(args[0])
    loaded = manifest.get("loaded_scripts")
    if loaded is None:
        return [entry, *includes], None
    if (not isinstance(loaded, list) or not loaded or not all(isinstance(n, str) for n in loaded)
            or loaded[0].translate(_ASCII_CASE) != entry.translate(_ASCII_CASE)):
        raise ValueError(f"{entry}: invalid recorder loaded_scripts trace {loaded!r}")
    return [entry, *(includes or loaded[1:])], [n.translate(_ASCII_CASE) for n in loaded]


def _same_bytes(path, data):
    try:
        return path.read_bytes() == data
    except FileNotFoundError:
        return False


def write_if_changed(path, data):
    """Concurrent map exports may publish the same shared body."""
    if _same_bytes(path, data):
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=path.parent) as directory:
        temp = Path(directory) / "body"
        temp.write_bytes(data)
        temp.replace(path)


@dataclass
class ScriptChain:
    path: str
    manifest: dict
    bodies: dict

    def record(self):
        return {"scriptManifest": self.path, "scripts": self.manifest["scripts"]}

    def products(self):
        yield from self.bodies.items()
        yield self.path, json.dumps(self.manifest, indent=2).encode("utf-8")

    def write(self, root):
        for name, data in self.products():
            write_if_changed(root / name, data)

    def complete(self, root, mission):
        try:
            world = json.loads((root / "maps" / self.manifest["map"] / "map.json").read_bytes())
            record = world["missions"][mission]
            return (all(record.get(k) == v for k, v in self.record().items())
                    and all(_same_bytes(root / name, data) for name, data in self.products()))
        except (FileNotFoundError, KeyError, AttributeError, TypeError, ValueError):
            return False


def script_chain(map_id, manifest, index, archive="mission.lvl"):
    """Resolve requests against the archive supplied to the recorder."""
    requests, loaded = _requests(manifest)
    entry = requests[0].translate(_ASCII_CASE)
    match = re.fullmatch(re.escape(map_id) + r"([a-z])_([a-z0-9_]+)", entry)
    era = profile(SWBF2).mission_eras.get(match[1]) if match else None
    mode = match[2] if era else None
    # Explicit manifests can also use entry points outside the map naming convention.
    stem = f"{era}-{mode}" if era else "entry-" + hashlib.sha256(entry.encode()).hexdigest()
    trace, records, bodies = [], [], {}
    cursor = 0
    for name in requests:
        key = name.translate(_ASCII_CASE)
        observed = loaded is None or cursor < len(loaded) and loaded[cursor] == key
        if observed:
            cursor += 1
        body = index.get(key)
        record = {"name": name, "id": key, "archive": archive, "file": None, "length": None, "sha256": None}
        if body is None:
            record["error"] = f"No script body for {name!r} in {archive}"
        elif not body.startswith(b"\x1bLua\x50"):
            record["error"] = f"Script {name!r} in {archive} has no compiled Lua 5.0 body"
        elif not observed:
            record["error"] = f"The recorder did not load {name!r} from {archive}"
        else:
            digest = hashlib.sha256(body).hexdigest()
            file = f"scripts/{digest}.luac"
            record.update(file=file, length=len(body), sha256=digest)
            bodies[file] = body
            trace.append(key)
        records.append(record)
    if loaded is not None and cursor != len(loaded):
        raise ValueError(f"{entry}: recorder loads absent from the ordered requests: {loaded[cursor:]!r}")
    document = {"version": 1, "family": SWBF2, "map": map_id, "era": era, "mode": mode,
                "entry": entry, "chunks": trace, "scripts": records}
    return ScriptChain(f"maps/{map_id}/scripts/{stem}.json", document, bodies)
