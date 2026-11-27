"""Refresh a 2004 sound manifest from the installation without encoding any samples."""

import errno
import json
import os
from pathlib import Path
import struct
import sys

from ucfb import chunks, iter_archives
from zero_addon import data_file, mission_root, missions
from zero_hash import fnv1a
from zero_paths import GAME, PUBLIC, child
import zero_sound as sound


# The sound step's version is recognizable in the data itself. The shared
# preparation receipt and all gameplay versions stay compatible.
SOUND_MANIFEST_VERSION = 3


def manifest_version(record):
    if (not isinstance(record, dict) or "map" not in record or not isinstance(record.get("sublevel"), str)
            or not isinstance(record.get("objects"), list) or not isinstance(record.get("mission"), dict)):
        raise ValueError("Invalid sound manifest")
    for obj in record["objects"]:
        if not isinstance(obj, dict) or not isinstance(obj.get("type"), str):
            raise ValueError("Invalid sound object")
        if obj["type"] != "SoundParameterized":
            continue
        layers = obj.get("Layer.children", [])
        if not isinstance(layers, list) or not all(isinstance(layer, dict) for layer in layers):
            raise ValueError("Invalid sound parameter layers")
        if ("Layer" in obj and "Layer.children" not in obj
                or any("ParameterGraphs" not in layer for layer in layers)):
            return 1
    if record.get("map") is not None and "lowReinforcementsVo" not in record["mission"]:
        return 2
    return 3


def source_objects(record, missions, game, cache):
    if "core" not in cache:
        core = (child(game, "core.lvl") or game / "core.lvl").read_bytes()
        configs = [(tag.decode(), b, e) for start, end in iter_archives(core)
                   for tag, b, e in chunks(core, start, end) if tag in (b"snd_", b"mus_", b"ffx_")]
        cache["core"] = sound.config_objects(core, configs)
    if record["sublevel"] == "shell" and record.get("map") is None:
        from zero_shell import archives
        archive = archives(game)["sound"]
    else:
        bank = record["mission"]["banks"][0]
        root = mission_root(missions[0], game) if bank.lower().startswith("dc:") and missions else None
        archive = data_file(bank, root, game)
        if archive is None:
            raise FileNotFoundError(f"Could not locate sound bank {bank}")
    if cache.get("archive") != archive:
        data = archive.read_bytes()
        cache.update(archive=archive, data=data, contents=sound.archive_contents(data))
    data, contents = cache["data"], cache["contents"]
    configs = contents["configs"]
    if record.get("map") is not None:
        span = contents["sublevels"][fnv1a(record["sublevel"])]
        configs = sound.sublevel_contents(data, span)["configs"]
    return cache["core"] + sound.config_objects(data, configs)


def update_curves(record, source):
    if len(record["objects"]) != len(source):
        raise ValueError("Sound bank contains a different set of objects")
    for old, new in zip(record["objects"], source):
        if (old["type"], old.get("Name")) != (new["type"], new.get("Name")):
            raise ValueError("Sound bank object identities differ")
        if new["type"] == "SoundParameterized" and "Layer.children" in new:
            existing = iter(old.get("Layer.children", []))
            updated = []
            for layer in new["Layer.children"]:
                fields = {k: v for k, v in layer.items() if k != "ParameterGraphs"}
                # The original writer stored a scope containing only one kind
                # of leaf under Layer itself. Its new parallel row is additive.
                if len(fields) == 1 and isinstance(next(iter(fields.values())), list) and "Layer" in old:
                    updated.append(layer)
                    continue
                original = next(existing, None)
                if (original is None or original.get("Name") != fields.get("Name")
                        or original.get("ParameterGraph") != fields.get("ParameterGraph")):
                    raise ValueError("Sound bank parameter layers differ from the prepared bank")
                updated.append({**original, "ParameterGraphs": layer["ParameterGraphs"]})
            if next(existing, None) is not None:
                raise ValueError("Prepared sound has extra parameter layers")
            old["Layer.children"] = updated


def refresh(audio, game, damaged=None):
    if not audio.is_dir() or not os.access(audio, os.W_OK):
        return 0
    index = audio / "index.json"
    try:
        by_mission = json.loads(index.read_bytes())["missions"] if index.is_file() else {}
        if not isinstance(by_mission, dict) or not all(isinstance(v, str) for v in by_mission.values()):
            raise ValueError("Invalid sound index")
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"Could not read {index}: {error}")
        if damaged is not None:
            damaged.add(None)
        by_mission = {}
    cache = {}
    changed = 0
    paths = sorted(audio.glob("*/sound.json"))
    if damaged is not None:
        damaged.update(set(by_mission.values()) - {path.parent.name for path in paths})
    for path in paths:
        try:
            record = json.loads(path.read_bytes())
            version = manifest_version(record)
            if "samples" in record:
                spans = record["samples"].values()
                length = max(((offset + frames * 2 + 3) // 4 * 4 for offset, frames, _ in spans), default=0)
                if path.with_name("bank.bin").stat().st_size != length:
                    raise ValueError("The prepared bank length differs from its sample spans")
        except (OSError, ValueError, TypeError, KeyError, AttributeError) as error:
            print(f"Could not read {path}: {error}")
            if damaged is not None:
                damaged.add(path.parent.name)
            continue
        if version >= SOUND_MANIFEST_VERSION or not os.access(path.parent, os.W_OK):
            continue
        names = [name for name, sub in by_mission.items() if sub == record["sublevel"]]
        try:
            if version < 2:
                update_curves(record, source_objects(record, names, game, cache))
            if record.get("map") is not None and "lowReinforcementsVo" not in record["mission"]:
                if "scripts" not in cache:
                    cache["scripts"] = missions(game)
                rules = sound.rules_for_sublevel(names, cache["scripts"], record["sublevel"])
                if not rules:
                    raise ValueError("No matching mission script for the prepared sound bank")
                # Map preparation writes historical battles first, then the regular battles in name order.
                default = max(rules, key=lambda name: (not name.endswith("_h"), name))
                record["mission"].update(sound.voice_setup(rules, default))
            sound.write_manifest(path, record)
            changed += 1
        except (OSError, LookupError, ValueError, TypeError, AttributeError, struct.error) as error:
            # An add-on or its matching source bank may have been removed.
            print(f"Kept {path}: {error}")
    return changed


def prepared_game():
    game = GAME
    receipt = PUBLIC / ".prepare.json"
    if child(game, "core.lvl") is None and receipt.is_file():
        try:
            old_game = json.loads(receipt.read_bytes())["game"]
        except (OSError, ValueError, TypeError, KeyError):
            old_game = None
        if isinstance(old_game, str) and old_game:
            game = Path(old_game)
    return game


def refresh_prepared(*, locked=False, damaged=None, index_only=False):
    import zero_family
    if zero_family.is_swbf2() or not (PUBLIC / "audio").is_dir() or not os.access(PUBLIC, os.W_OK):
        return 0
    game = prepared_game()
    if child(game, "core.lvl") is None:
        print("Sound manifests can be updated when the game installation is available.")
        return 0
    # A full preparation already owns this lock. Background maintenance yields
    # to an active writer, and leave read-only portable content usable.
    handle = None
    try:
        if not locked:
            from prepare import hold_lock
            try:
                handle = hold_lock(PUBLIC / ".prepare.lock")
            except OSError as error:
                if error.errno in (errno.EACCES, errno.EROFS, errno.EPERM):
                    return 0
                raise
            if handle is None:
                return 0
        from zero_sound_index import restore
        restore(PUBLIC, game)
        if index_only:
            return 0
        changed = refresh(PUBLIC / "audio", game, damaged)
        print(f"Updated {changed} sound manifests.")
        return changed
    finally:
        if handle:
            handle.close()


if __name__ == "__main__":
    try:
        refresh_prepared()
    except (OSError, ValueError, KeyError) as exc:
        sys.exit(f"Could not refresh sound manifests: {exc}")
