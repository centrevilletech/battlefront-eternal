"""The 2005 receipt's inventory of prepared files."""

from contextlib import contextmanager
import hashlib
import json
import os
import ntpath
from pathlib import Path, PurePosixPath, PureWindowsPath
import re
import stat
import shutil
import tempfile

from zero_sound_health import old_manifest_directory, original_part

MACHINERY = {".prepare.lock", ".locks", ".prepare-swbf2.json", ".prepare-swbf2.tmp", ".prepare.json",
             ".prepare.tmp", ".bfe-family.json", ".bfe-import.json"}
BODY = re.compile(r"scripts/([0-9a-f]{64})\.luac")
HUD_IMAGE = re.compile(r"maps/[^/]+/hud/([0-9a-f]{64})\.png")
RECORDS = {"map": "map.json", "side": "units.json", "common": "common.json"}


class PayloadReadError(OSError):
    """An interior read failed after the receipts and marker identified the 2005 tree."""


def destination_origin(root, *, initializing=False, client_handover=None):
    """Identify the destination before opening its lock, and again while holding it."""
    def refuse(detail):
        raise ValueError(f"Cannot prepare the 2005 game in {root}: {detail}. The folder was left intact.")

    def record(path):
        check_existing(path, directory=False)
        try:
            return json.loads(path.read_bytes())
        except (ValueError, UnicodeError):
            return None

    from prepare import COMMON_VERSION, MAP_IMPORT_VERSION, SIDE_EXPORT_VERSION, STATE_VERSION
    from zero_mission2 import PREPARED_VERSION
    check_existing(root, directory=True)
    if not root.exists():
        return "empty"
    names = {p.name for p in root.iterdir()}
    for name in (".prepare.lock", ".prepare-swbf2.json", ".prepare.json", ".bfe-family.json"):
        check_existing(root / name, directory=False)
    if not names or initializing and names == {".prepare.lock"}:
        return "empty"
    if (names == {".prepare.lock", ".bfe-family.json"}
            and record(root / ".bfe-family.json") == {"family": "swbf2", "format": 1}
            and (initializing or client_handover and client_handover(root / ".prepare.lock"))):
        return "empty"
    if ".bfe-family.json" in names:
        marker = record(root / ".bfe-family.json")
        if isinstance(marker, dict) and marker.get("family", "swbf2") != "swbf2":
            refuse("the marker names a different game")
    origin = None
    for name in (".prepare.json", ".prepare-swbf2.json"):
        if name not in names:
            continue
        data = record(root / name)
        if not isinstance(data, dict):
            refuse(f"{name} is not a 2005 record")
        stamps = ("version", "mapImportVersion", "muzzleVersion", "commonVersion")
        if (name == ".prepare.json" and data.get("complete") is True
                and all(type(data.get(k)) is int for k in stamps)
                and tuple(data[k] for k in stamps) == (STATE_VERSION, MAP_IMPORT_VERSION, SIDE_EXPORT_VERSION, COMMON_VERSION)
                and data.get("skipped") == [] and data.get("failed") == []):
            refuse("the released game accepts .prepare.json as a current 2004 record")
        generation = data.get("swbf2Version")
        if data.get("family") != "swbf2" or type(generation) is not int or not 0 < generation <= PREPARED_VERSION:
            refuse(f"{name} does not identify a supported 2005 generation")
        if name == ".prepare.json":
            game = data.get("game")
            if (data.get("version") != STATE_VERSION or not isinstance(game, str)
                    or not (PurePosixPath(game).is_absolute() or PureWindowsPath(game).is_absolute())
                    or type(data.get("complete")) is not bool or not isinstance(data.get("done"), list)
                    or not all(isinstance(step, str) for step in data["done"])):
                refuse("the old record does not match an early 2005 preparation")
            origin = "early"
        elif not origin:
            origin = "dedicated"
    if not origin:
        refuse("the nonempty folder has no 2005 preparation record")
    read_error = None

    @contextmanager
    def check_payload():
        nonlocal read_error
        try:
            yield
        except OSError as error:
            # Continue checking other products. A foreign product vetoes even a lock probe.
            if read_error is None:
                read_error = error

    # A genuine receipt cannot authorize overwriting a different game's payload.
    for folder, leaf, released_keys in (("common", "common.json", ("holograms", "rasterHud")),
                                         ("maps", "map.json", ("world", "missions")),
                                         ("sides", "units.json", ("firstPersonBones", "wantedEffects"))):
        parent = root / folder
        with check_payload():
            check_existing(parent, directory=True)
            if not parent.is_dir():
                continue
            if folder == "common":
                if "shell" in {p.name for p in parent.iterdir()}:
                    refuse("the folder contains the 2004 shell")
                rows = [parent]
            else:
                rows = list(parent.iterdir())
            for directory in rows:
                with check_payload():
                    check_existing(directory, directory=True)
                    path = directory / leaf
                    check_existing(path, directory=False)
                    if path.is_file():
                        data = record(path)
                        if isinstance(data, dict) and (data.get("family", "swbf2") != "swbf2"
                                or "family" not in data and all(k in data for k in released_keys)):
                            refuse(f"{path.relative_to(root)} identifies 2004 content")
    if read_error is not None:
        raise PayloadReadError(str(read_error)) from read_error
    return origin


def _same_game(saved, game):
    """The recorded folder is this one: an absolute spelling of the same
    folder (the same spelling, or reached by another: a link, another case
    or separator on a folding disk, an 8.3 alias), as the client's own
    record check reads it. A relative or unreadable spelling names no copy
    here, and a damaged one only loses the resume, never the run."""
    if not isinstance(saved, str) or not saved or not os.path.isabs(saved):
        return False
    if saved == str(game):
        return True
    try:
        return os.path.samefile(saved, game)
    except (OSError, ValueError):
        return False


class Receipt:
    """Read only the fields that affect a 2005 resume. Publish a stable, atomic receipt."""

    def __init__(self, path, game):
        from prepare import COMMON_VERSION, MAP_IMPORT_VERSION, SIDE_EXPORT_VERSION, STATE_VERSION
        self.path = path
        try:
            saved = json.loads(path.read_bytes())
        except (FileNotFoundError, ValueError):
            saved = {}
        if (not isinstance(saved, dict) or saved.get("family") != "swbf2" or not _same_game(saved.get("game"), game)
                or saved.get("version") != STATE_VERSION or not isinstance(saved.get("done"), list)
                or not all(isinstance(k, str) for k in saved["done"])):
            saved = {}
        done = saved.get("done", [])
        for field, version, prefix in (("mapImportVersion", MAP_IMPORT_VERSION, "map:"),
                                       ("muzzleVersion", SIDE_EXPORT_VERSION, "side:"),
                                       ("commonVersion", COMMON_VERSION, "common")):
            if saved.get(field) != version:
                done = [k for k in done if not k.startswith(prefix)]
        recorded_game = saved.get("game") if saved else str(game)
        self.data = dict(version=STATE_VERSION, game=recorded_game, family="swbf2", done=done,
                         mapImportVersion=MAP_IMPORT_VERSION, muzzleVersion=SIDE_EXPORT_VERSION,
                         commonVersion=COMMON_VERSION, swbf2Version=saved.get("swbf2Version"),
                         missionInputs=saved.get("missionInputs"), outputs=saved.get("outputs"),
                         previousOutputs=saved.get("previousOutputs"),
                         addOns=[], skippedAddOns=[], failed=[], skipped=[], failures=[])

    def done(self, key):
        return key in self.data["done"]

    def mark(self, key):
        self.data["done"].append(key)
        self.save()

    def save(self, **fields):
        self.data.update(fields)
        data = json.dumps(self.data, indent=1, sort_keys=True).encode("utf-8")
        if self.path.exists() and self.path.read_bytes() == data:
            return
        self.path.parent.mkdir(parents=True, exist_ok=True)
        temp = self.path.with_suffix(".tmp")
        temp.unlink(missing_ok=True)
        with temp.open("xb") as target:
            target.write(data)
        temp.replace(self.path)


def workspace(root):
    """A surviving extractor can only write into its own run's working directory."""
    locks = root / ".locks"
    locks.mkdir(parents=True, exist_ok=True)
    return tempfile.TemporaryDirectory(prefix="products-", dir=locks)


def clear_workspaces(root):
    locks = root / ".locks"
    if locks.is_dir():
        for path in locks.iterdir():
            if re.fullmatch(r"products-[a-z0-9_]{8}", path.name) and path.is_dir():
                shutil.rmtree(path)


def check_kind(path, info):
    if (getattr(info, "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT
            or not (stat.S_ISDIR(info.st_mode) or stat.S_ISREG(info.st_mode))):
        raise ValueError(f"Expected a plain file or folder at {path}")


def check_existing(path, *, directory):
    try:
        info = path.lstat()
    except FileNotFoundError:
        return
    check_kind(path, info)
    if directory != stat.S_ISDIR(info.st_mode):
        raise ValueError(f"Unexpected file type at {path}")


def _reserved(name):
    return ntpath.isreserved(name) if hasattr(ntpath, "isreserved") else PureWindowsPath(name).is_reserved()


def component(name):
    if (not isinstance(name, str) or not name or name in (".", "..") or name.endswith((" ", "."))
            or any(ord(c) < 32 or c in '/\\:<>"|?*' for c in name) or _reserved(name)):
        raise ValueError(f"Invalid prepared file name {name!r}")
    return name


def relative_name(name):
    if not isinstance(name, str):
        raise ValueError("Invalid prepared path")
    for part in name.split("/"):
        component(part)
    return name


def group(rel):
    top, *parts = rel.split("/")
    low = top.lower()
    if (low in MACHINERY or low.startswith(".bfe-family.json.") and low.endswith(".tmp")
            or (top == "audio" and parts[:1] == ["enhanced"])):
        return None
    if top == "maps" and len(parts) >= 3 and parts[1] == "effects":
        return "effects:" + parts[0]
    if top in ("maps", "sides") and len(parts) >= 2:
        return ("map:" if top == "maps" else "side:") + parts[0]
    return "common" if top in ("common", "firstperson") else "*"


def interrupted_file(rel):
    return rel == ".prepare-swbf2.tmp" or original_part(rel)


def interrupted_directory(rel):
    if re.fullmatch(r"(?:scripts|maps/[^/]+/scripts)/tmp[a-z0-9_]{8}", rel):
        return "body"
    if old_manifest_directory(rel):
        return "sound.json"
    return None


def inventory(root, *, verify_links=True, leftovers=None):
    """Read names from disk. Receipt strings are never used for filesystem access."""
    result = {}
    seen = set()

    def visit(folder):
        with os.scandir(folder) as children:
            for child in children:
                path = Path(child.path)
                rel = path.relative_to(root).as_posix()
                check_kind(path, info := child.stat(follow_symlinks=False))
                if rel.lower() == "bfe-content-manifest.json":
                    raise ValueError(f"Reserved archive name in prepared content: {rel}")
                if interrupted_file(rel) and stat.S_ISREG(info.st_mode):
                    if leftovers is not None:
                        leftovers.append(path)
                    continue
                if stat.S_ISDIR(info.st_mode) and (temp_name := interrupted_directory(rel)):
                    contents = list(path.iterdir())
                    if all(item.name == temp_name and stat.S_ISREG(item.lstat().st_mode) for item in contents):
                        for item in contents:
                            check_kind(item, item.lstat())
                        if leftovers is not None:
                            leftovers.extend([*contents, path])
                        continue
                key = group(rel)
                if key is None and not verify_links:
                    continue
                if key is not None:
                    component(child.name)
                    if rel.lower() in seen:
                        raise ValueError(f"Duplicate prepared path {rel}")
                    seen.add(rel.lower())
                if stat.S_ISDIR(info.st_mode):
                    visit(path)
                elif key is not None:
                    if rel.startswith("scripts/") and not BODY.fullmatch(rel):
                        raise ValueError(f"Expected a content-addressed script body at {rel}")
                    row = [rel, info.st_size]
                    if BODY.fullmatch(rel) or rel.startswith("firstperson/"):
                        with path.open("rb") as payload:
                            row.append(hashlib.file_digest(payload, "sha256").hexdigest())
                    result.setdefault(key, []).append(row)

    check_existing(root, directory=True)
    if root.exists():
        visit(root)
    return {key: sorted(rows) for key, rows in result.items()}


def matches(rows, actual):
    """Order is immaterial. Sizes are integers, script hashes mandatory and content-addressed."""
    if not isinstance(rows, list) or len(rows) != len(actual):
        return False
    expected = {row[0]: row for row in actual}
    for row in rows:
        if not isinstance(row, list) or len(row) not in (2, 3):
            return False
        try:
            relative_name(row[0])
        except ValueError:
            return False
        if type(row[1]) is not int or row[1] < 0 or expected.pop(row[0], None) != row:
            return False
        if len(row) == 3 and not row[0].startswith("firstperson/") and row[0] != f"scripts/{row[2]}.luac":
            return False
    return not expected


def private_record(key):
    if key == "common":
        return "common/common.json"
    kind, _, name = key.partition(":")
    return f"{'maps' if kind == 'map' else 'sides'}/{name}/{RECORDS[kind]}"


def core_present(key, rows, record=None):
    sizes = dict((row[0], row[1]) for row in rows)
    required = [record or private_record(key)]
    if key == "common":
        required.extend(("firstperson/manifest.json", "firstperson/anims.bin"))
    if key.startswith("map:"):
        required.extend(f"maps/{key[4:]}/{rel}" for rel in ("geom.bin", "hud/catalog.json"))
    return all(sizes.get(path, 0) > 0 for path in required)


def named_image_matches(path):
    check_existing(path, directory=False)
    # RGBA at the published pixel limit plus PNG rows and container overhead.
    return (0 < path.stat().st_size <= (17 << 20)
            and hashlib.sha256(path.read_bytes()).hexdigest() == path.stem)


def shared_present(root, rows, banks):
    """Check the required bank, stream index and stored sample spans against plain files we enumerated."""
    if not banks:
        return True
    sizes = {row[0]: row[1] for row in rows}
    if not sizes.get("audio/index.json"):
        return False
    def read(name):
        if sizes.get(name, 0) <= 0:
            raise ValueError(f"Missing required sound record {name}")
        return json.loads((root / name).read_bytes())
    try:
        streams = set()
        for name in banks:
            bank = "audio/" + component(name)
            sound = read(bank + "/sound.json")
            length = sizes[bank + "/bank.bin"]
            if any(offset + frames * 2 > length for offset, frames, _ in sound["samples"].values()):
                return False
            streams.update(component(s) for s in sound["streams"])
        for name in streams:
            stream = "audio/streams/" + name
            length = sizes[stream + ".bin"]
            if any(offset + size > length for offset, size, _, _ in read(stream + ".json")["segments"].values()):
                return False
    except (KeyError, TypeError, AttributeError, ValueError):
        return False
    return True


def first_person_complete(root, rows):
    sizes = {r[0]: r[1] for r in rows}
    try:
        if not 0 < sizes.get("firstperson/manifest.json", 0) <= 8 << 20:
            return False
        manifest = json.loads((root / "firstperson/manifest.json").read_bytes())
        if manifest.get("version") != 2:
            return False
        names = [manifest["animsData"], *manifest["textures"].values()]
        for rig in manifest["rigs"].values():
            rel = rig["file"]
            if not rel.endswith(".json"):
                return False
            stem = rel[:-5]
            names.extend((rel, stem + ".bin", stem + "_index.bin"))
        return all(sizes.get("firstperson/" + relative_name(rel), 0) > 0 for rel in names)
    except (AttributeError, KeyError, OSError, TypeError, ValueError):
        return False


def publish(root, work, key, rows, previous, recorded, *, record=None):
    """Install only files actually emitted in an empty work folder on the same volume."""
    by_name = {row[0]: row for row in rows}
    if key == "common" and not first_person_complete(work, rows):
        raise ValueError("Incomplete first-person files for common.")
    if key != "*" and not core_present(key, rows, record):
        raise ValueError(f"It finished without writing nonempty required files for {key}.")
    retired = []
    known = {r[0]: r for r in recorded if isinstance(r, list) and r and isinstance(r[0], str)} if isinstance(recorded, list) else {}
    for row in previous:
        if row[0] not in by_name:
            # Preserve additional content outside the extractors' directories for export.
            if key == "*" and row[0].split("/", 1)[0] not in ("audio", "scripts"):
                by_name[row[0]] = row
            elif BODY.fullmatch(row[0]) and matches([known.get(row[0])], [row]):
                retired.append(row[0])
            elif (HUD_IMAGE.fullmatch(row[0]) and group(row[0]) == key
                  and matches([known.get(row[0])], [row]) and named_image_matches(root / row[0])):
                retired.append(row[0])
            else:
                raise ValueError(f"Existing file was not produced by this preparation: {row[0]}")
    for row in rows:
        rel = row[0]
        if len(row) == 3 and not rel.startswith("firstperson/") and rel != f"scripts/{row[2]}.luac":
            raise ValueError(f"Script body does not match its SHA-256 name: {rel}")
    # Install the resources an effect pack references before exposing its descriptor.
    ordered = sorted(rows, key=lambda row: row[0] == record) if record else rows
    for rel, *_ in ordered:
        target = root / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        (work / rel).replace(target)
    for rel in retired:
        (root / rel).unlink()
    return sorted(by_name.values())
