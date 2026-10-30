"""Add-ons: maps installed beside the game in GameData/AddOn/<name>/, as the
Steam copy installs Jabba's Palace (AddOn/Tat3). docs/formats.md has the
layout.

An add-on folder holds addme.script, a compiled Lua chunk the game runs at
startup, and Data/_LVL_PC laid out like the game's own. The script appends
the add-on's maps to the shell's lists (sp_missionselect_listbox_contents
and mp_missionselect_listbox_contents, entries like the stock missionlist's)
and registers their missions (AddDownloadableContent). Data/_LVL_PC holds
mission.lvl (its missions), core.lvl (the localization, its own strings
added), Shell/<language>/shell.lvl (its flyover's movie config),
Movies/*.mvs, the world in a planet folder, Side/<side>.lvl for armies of
its own and sound/<planet>.lvl. Its mission scripts name their own files
with "dc:" before them ("dc:TAT\\tat3.lvl", "dc:sound\\tat.lvl;tat3cw")
and the game's without ("SIDE\\all.lvl", the era's music in
"sound\\tat.lvl"). Names match without regard to case, as the game's
loader does on Windows (Tat3 spells them _lvl_pc and Side).

The tools read the installation through the functions here, so an add-on
is found, cataloged and extracted like the stock maps, whatever its name.
An add-on whose own files can't be read (tools/prepare.py checks each one
before a run) is left out of the run: BFE_SKIP_ADDONS names it, and then
none of the functions here returns it or anything of its.
"""

import os
import re
import struct
from collections import namedtuple
from pathlib import Path

from zero_locl import localization as archive_localization
from zero_lua import calls, getn, global_tables, load, mission_scripts
from zero_paths import GAME

AddOn = namedtuple("AddOn", "name folder root script")
# The shell's lists of maps an addme script appends to: Instant Action's
# (and the singleplayer screens'), and multiplayer's.
LISTS = ("sp_missionselect_listbox_contents", "mp_missionselect_listbox_contents")


def child(base, *parts):
    """base/parts, each part matched without regard to case where the exact
    spelling isn't there, and spelled as on disk; None when one is missing.
    On disk because outputs record these paths (source_name): Windows finds
    "_LVL_PC" and "SIDE" in AddOn/Tat3, which spells them "_lvl_pc" and
    "Side", and must record what Linux does, or the content fingerprint
    differs (src/runtime/content-hash.js)."""
    at = Path(base)
    for part in parts:
        try:
            names = [entry.name for entry in at.iterdir()]
        except OSError:
            return None
        name = part if part in names else next((name for name in names if name.lower() == part.lower()), None)
        if name is None:
            return None
        at = at / name
    return at


def skipped():
    """The add-ons this run leaves out (BFE_SKIP_ADDONS: their names, lower
    case, separated by commas)."""
    return {name.strip().lower() for name in os.environ.get("BFE_SKIP_ADDONS", "").split(",") if name.strip()}


def add_ons(game=GAME, everything=False):
    """The installed add-ons in name order: (name, folder, its Data/_LVL_PC,
    its addme.script). game is the stock Data/_LVL_PC; the add-ons are in
    GameData/AddOn beside it. A folder without addme.script or Data/_LVL_PC
    isn't one: the game skips it. Those the run leaves out (skipped) are
    not returned unless everything is asked for."""
    found = []
    folder = child(Path(game).parents[1], "AddOn")
    try:
        entries = sorted((entry for entry in folder.iterdir() if entry.is_dir()), key=lambda p: p.name.lower()) if folder else []
    except OSError:
        entries = []
    left_out = set() if everything else skipped()
    for entry in entries:
        script, root = child(entry, "addme.script"), child(entry, "Data", "_LVL_PC")
        if script and root and entry.name.lower() not in left_out:
            found.append(AddOn(entry.name.lower(), entry, root, script))
    return found


def data_file(path, root=None, game=GAME):
    """The file a mission script names: "dc:..." in its add-on's folder
    (root, the add-on's Data/_LVL_PC), anything else in the game's; a
    ";sublevel" dropped. None when it isn't there."""
    path = path.split(";", 1)[0]
    base = game
    if path[:3].lower() == "dc:":
        path, base = path[3:], root
    parts = [part for part in path.replace("\\", "/").split("/") if part]
    return child(base, *parts) if base is not None and parts else None


def map_archive(name, game=GAME):
    """A map's world, <PLANET>/<map>.lvl: the game's (bes2 is BES/bes2.lvl),
    else an add-on's (Jabba's Palace is TAT/tat3.lvl in AddOn/Tat3), else
    the world the map's own battles read, whatever its name (an add-on
    listing "georuins" whose battles read GEO\\geo1.lvl). The game's path
    when none has it, for the message."""
    stock = Path(game) / name[:3].upper() / f"{name}.lvl"
    if stock.exists():
        return stock
    for add_on in add_ons(game):
        found = child(add_on.root, name[:3], f"{name}.lvl")
        if found:
            return found
    return battle_world(name, game) or stock


def world_read(chunk):
    """The world a mission script reads: its last ReadDataFile of
    <folder>\\<file>.lvl outside SIDE and sound ("GEO\\geo1.lvl",
    "dc:TAT\\tat3.lvl"); None without one."""
    world = None
    for call, args in calls(load(chunk)):
        if call != "ReadDataFile" or not args or not isinstance(args[0], str):
            continue
        path = args[0].split(";", 1)[0]
        parts = [part for part in (path[3:] if path[:3].lower() == "dc:" else path).replace("/", "\\").split("\\") if part]
        if len(parts) == 2 and parts[0].lower() not in ("side", "sound") and parts[1].lower().endswith(".lvl"):
            world = path
    return world


def battle_world(name, game=GAME):
    """The world archive a map's battles read (its regular ones first, then
    the campaign's), found as data_file finds it; None when no battle names
    one that is there."""
    found = missions(game)
    battles = sorted((m for m in found if re.fullmatch(re.escape(name.lower()) + r"[a-z](_h)?", m.lower())),
                     key=lambda m: (m.lower().endswith("_h"), m))
    for mission in battles:
        try:
            world = world_read(found[mission])
        except (ValueError, IndexError, struct.error):
            continue
        path = world and data_file(world, mission_root(mission, game), game)
        if path and path.is_file():
            return path
    return None


def side_archive(side, game=GAME):
    """A side's archive, SIDE/<side>.lvl: the game's, else an add-on's own
    army (Jabba's Palace's Gamorreans are Side/gam.lvl)."""
    stock = Path(game) / "SIDE" / f"{side}.lvl"
    if stock.exists():
        return stock
    for add_on in add_ons(game):
        found = child(add_on.root, "SIDE", f"{side}.lvl")
        if found:
            return found
    return stock


def mission_files(game=GAME):
    """Each mission.lvl with the folder its "dc:" paths are in: the game's
    (None), then each add-on's."""
    files = [(Path(game) / "mission.lvl", None)]
    files += [(child(add_on.root, "mission.lvl"), add_on.root) for add_on in add_ons(game)]
    return [(path, root) for path, root in files if path and path.exists()]


def missions(game=GAME):
    """Every mission script, name -> compiled chunk: mission.lvl's in file
    order, then each add-on's (Jabba's Palace's tat3a and tat3c). A name
    the game's has keeps the game's."""
    found = {}
    for path, _ in mission_files(game):
        for name, chunk in mission_scripts(path.read_bytes()).items():
            found.setdefault(name, chunk)
    return found


def mission_root(mission, game=GAME):
    """The add-on folder whose mission.lvl holds a mission (where its "dc:"
    paths are), or None for the game's own."""
    for path, root in mission_files(game):
        if mission in mission_scripts(path.read_bytes()):
            return root
    return None


def localization(game=GAME, language="english"):
    """The game's text (core.lvl's Locl) with what each add-on's core.lvl
    adds: Jabba's Palace's command post names and objectives
    (level.tat3.*). A key the game's has keeps the game's text."""
    table = {}
    for path in [Path(game) / "core.lvl"] + [child(add_on.root, "core.lvl") for add_on in add_ons(game)]:
        if path and path.exists():
            for key, text in archive_localization(path.read_bytes(), language).items():
                table.setdefault(key, text)
    return table


def add_on_maps(add_on, key=LISTS[0]):
    """The entries one add-on's addme script appends to one of the shell's
    lists (listed_maps); raises when the script can't be read."""
    out = []
    for chunk in mission_scripts(add_on.script.read_bytes()).values():
        found = global_tables(load(chunk), env={name: {} for name in LISTS}, builtins={"getn": getn})
        out += [{**item, "addOn": add_on.name} for item in found.get(key) or []
                if isinstance(item, dict) and isinstance(item.get("mapluafile"), str)]
    return out


def listed_maps(game=GAME, key=LISTS[0]):
    """The entries the add-ons' addme scripts append to one of the shell's
    lists, in add-on order: the stock missionlist's fields (mapluafile,
    showstr, side_<letter>) and the add-on's name ("addOn"). Each script
    runs against empty lists, with getn, as the shell runs it."""
    out = []
    for add_on in add_ons(game):
        try:
            out += add_on_maps(add_on, key)
        except (OSError, ValueError, IndexError, struct.error):
            print(f"  add-on {add_on.name}: addme.script unreadable", flush=True)
    return out


def movie_archives(game=GAME, language="ENG"):
    """Each add-on's movies (its maps' Instant Action flyovers) with the
    movie config naming them: [(Movies/<name>.mvs, Shell/<language>/shell.lvl
    or None)]."""
    found = []
    for add_on in add_ons(game):
        movies = child(add_on.root, "Movies")
        shell = child(add_on.root, "Shell", language, "shell.lvl")
        for path in sorted(movies.iterdir()) if movies and movies.is_dir() else []:
            if path.suffix.lower() == ".mvs":
                found.append((path, shell))
    return found


def source_name(path, game=GAME):
    """An archive's path as outputs record it: under the game's
    Data/_LVL_PC ("TAT/tat2.lvl"), else under GameData
    ("AddOn/Tat3/Data/_lvl_pc/TAT/tat3.lvl"), else its name."""
    for base in (Path(game), Path(game).parents[1]):
        try:
            return Path(path).relative_to(base).as_posix()
        except ValueError:
            pass
    return Path(path).name
