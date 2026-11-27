"""Reconstruct the 2004 battle-to-bank index from completed maps and their mission scripts."""
import json
from functools import lru_cache
import os
import re
import struct

from zero_addon import missions
from zero_lua import calls, load
from zero_sound import read_mission_audio
import zero_family


def records(root):
    receipt = root / ".prepare.json"
    try:
        done = json.loads(receipt.read_bytes())["done"]
    except FileNotFoundError:
        return []
    except (KeyError, TypeError, ValueError):
        done = []
    if not isinstance(done, list):
        done = []
    names = {key[4:] for key in done if isinstance(key, str) and key.startswith("map:")}
    # An interrupted preparation may have unmarked a map while its complete world is still present.
    names.update(path.parent.name for path in (root / "maps").glob("*/map.json"))
    result = []
    for name in sorted(names):
        if not name or name in (".", "..") or any(c in name for c in "/\\:"):
            print(f"Skipping invalid prepared map name {name!r}")
            continue
        result.append((name, root / "maps" / name / "map.json"))
    return result


def map_banks(name, path, scripts, game):
    try:
        battles = json.loads(path.read_bytes())["missions"]
        local = {key: row["mission"] for key, row in battles.items() if not row["mission"].get("world")}
    except (OSError, ValueError, TypeError, KeyError, AttributeError):
        # The preparation receipt can survive a damaged world record. Use
        # the same world comparison as the map extractor, at the recorded install.
        from extract import mission_world, same_world
        from zero_addon import map_archive
        pattern = zero_family.profile().mission_pattern(name)
        local = {}
        for key, chunk in scripts.items():
            if re.fullmatch(pattern, key):
                world = mission_world(chunk)
                if not world or same_world(world, name, lambda name: map_archive(name, game)):
                    local[key] = {"historical": key.endswith("_h")}
        if not local:
            raise ValueError(f"Cannot identify the prepared missions of {name} from {path} or {game}")
    rows = {}
    # This is the order used by extract_map_audio when selecting the map's default bank.
    for key in sorted(local, key=lambda key: (not local[key].get("historical"), key)):
        if key not in scripts:
            raise ValueError(f"Cannot resolve prepared mission {key} in {game}")
        banks = read_mission_audio(calls(load(scripts[key])))["banks"]
        if banks:
            bank = banks[0].partition(";")[2]
            if bank:
                rows[key] = bank
    return rows


@lru_cache(maxsize=128)
def report_skipped_map(name, reason):
    print(f"Skipping sound index entries for {name}: {reason}")


def expected_index(maps, game):
    scripts = missions(game)
    by_map, by_mission = {}, {}
    for name, path in maps:
        try:
            rows = map_banks(name, path, scripts, game)
        except (OSError, ValueError, LookupError, TypeError, AttributeError, struct.error) as error:
            report_skipped_map(name, str(error))
            continue
        if rows:
            by_mission.update(rows)
            by_map[name] = next(reversed(rows.values()))
    return {"maps": by_map, "missions": by_mission}


def restore(root, game):
    """Under the tree lock, publish the full index once. A surviving candidate requests another check."""
    maps = records(root)
    if not maps:
        return
    audio = root / "audio"
    path = audio / "index.json"
    staged = audio / ".index-rebuild"
    desired = expected_index(maps, game)
    if not desired["missions"]:
        return
    try:
        previous = json.loads(path.read_bytes())
        if not isinstance(previous, dict) or not all(isinstance(previous.get(k), dict) for k in desired):
            previous = {}
    except (FileNotFoundError, ValueError):
        previous = {}
    complete = True
    for key, rows in desired.items():
        original = previous.get(key, {})
        complete = complete and key in previous and all(original.get(k) == v for k, v in rows.items())
        # Preserve entries for other, already prepared missions.
        desired[key] = {**original, **rows}
    if complete:
        staged.unlink(missing_ok=True)
        return
    # Its presence also records an interrupted rebuild. Recompute from the maps on retry.
    staged.unlink(missing_ok=True)
    with staged.open("x", encoding="utf-8", newline="\n") as pending:
        json.dump(desired, pending, indent=1, sort_keys=True)
        pending.flush()
        os.fsync(pending.fileno())
    staged.replace(path)
    print(f"Restored the sound index for {len(desired['missions'])} missions.")
