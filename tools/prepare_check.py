"""What preparation needs from an installation, looked at before anything is
converted: tools/prepare.py --check, which the desktop app runs for its
confirm screen (docs/desktop.md), and the add-on check every run starts with.

It goes through the tools' own paths (tools/zero_paths.py, zero_shell.py's
archives, zero_addon.py's maps, armies and add-ons), so it finds what a run
would trip over, in plain words: a folder or archive that is missing, empty,
not a game archive, or spelled otherwise than the tools spell it (on Linux
names must match exactly), English files missing (the tools read ENG), and
a game program the maps' painted foliage can't be placed from
(tools/zero_foliage.py). It can't tell an edited archive from the original:
that would take the originals' checksums, which this project doesn't keep.

Add-ons are looked at one by one. One whose own files can't be read is left
out of a run altogether (zero_addon.skipped); a map of one that needs a file
neither the game nor any add-on has is left out by itself. Neither stops the
rest of the game.
"""

import os
import re
import struct
from pathlib import Path

import zero_addon
from zero_locl import localization as read_localization
from zero_lua import calls, load, mission_scripts, mission_setup
from zero_paths import GAME

LANGUAGE = "ENG"  # the tools read the English files (zero_shell.py, extract.py)
ARCHIVE = b"ucfb"  # how every game archive starts (docs/formats.md)
# Whatever reading an archive the tools can't make sense of raises: a broken
# add-on (or a damaged archive) must not stop the check, only be reported.
UNREADABLE = Exception


def install_root(game):
    """The folder holding GameData, for game (GameData/Data/_LVL_PC)."""
    return Path(game).parents[2]


def shown(path, game):
    """A path as the player finds it, from GameData on."""
    try:
        return Path(path).relative_to(install_root(game)).as_posix()
    except ValueError:
        return Path(path).as_posix()


def spelled(path):
    """The path on disk that matches path without regard to case, or None:
    a copy whose folders lost the game's capitalization."""
    path = Path(path)
    at = Path(path.anchor)
    for part in path.parts[1:]:
        exact = at / part
        if exact.exists():
            at = exact
            continue
        try:
            at = next(entry for entry in at.iterdir() if entry.name.lower() == part.lower())
        except (OSError, StopIteration):
            return None
    return at


def problem(code, path, game, message):
    return {"code": code, "path": shown(path, game) if path is not None else None, "message": message}


def archive_problem(path, game, what=None):
    """None, or what is wrong with a game archive (or folder) a run reads:
    missing, named otherwise, empty, or not an archive at all."""
    path = Path(path)
    name = shown(path, game)
    about = f" ({what})" if what else ""
    try:
        if path.is_dir():
            return None
        if path.is_file():
            if path.stat().st_size == 0:
                return problem("empty", path, game, f"{name}{about} is empty.")
            with open(path, "rb") as handle:
                if handle.read(4) != ARCHIVE:
                    return problem("damaged", path, game, f"{name}{about} isn't a Star Wars Battlefront archive. It may be damaged.")
            return None
    except OSError as error:
        return problem("unreadable", path, game, f"{name}{about} can't be read ({error.strerror or error}).")
    found = spelled(path)
    if found and found != path:
        wanted, have = next((a, b) for a, b in zip(path.parts, found.parts) if a != b)
        return problem("misnamed", path, game, f"{name}{about} is spelled {have} in this copy, where it must be "
                                               f"{wanted}: on this computer the names have to match exactly.")
    return problem("missing", path, game, f"{name}{about} is missing.")


def unreadable(path, game, error):
    return problem("unreadable", path, game, f"{shown(path, game)} can't be read ({str(error) or type(error).__name__}).")


def tidy(problems):
    """Each problem once (many battles read the same army), none under a
    folder already reported missing or misnamed."""
    out, seen = [], set()
    for entry in sorted((entry for entry in problems if entry), key=lambda entry: len(entry["path"] or "")):
        key = (entry["code"], entry["path"], entry["message"])
        under = entry["path"] and any(entry["path"].startswith(other["path"] + "/") for other in out
                                      if other["path"] and other["code"] in ("missing", "misnamed", "language"))
        if key not in seen and not under:
            seen.add(key)
            out.append(entry)
    return sorted(out, key=lambda entry: problems.index(entry))


def battles_of(map_id, scripts):
    """A map's mission scripts: each era's battle and its campaign version
    (bes2r, bes2a_h), as tools/extract.py reads them."""
    return {name: chunk for name, chunk in scripts.items() if re.fullmatch(re.escape(map_id) + r"[a-z](_h)?", name.lower())}


def sound_bank(bank, root, game):
    """The sound archive a battle's first bank is in, found as
    tools/zero_sound.py's sound_file finds it."""
    return zero_addon.data_file(bank, root, game) or Path(game) / "Sound" / bank.split(";", 1)[0].split("\\")[-1]


def mission_roots(game):
    """Each mission's add-on folder (where its "dc:" files are), None for
    the game's own: zero_addon.mission_root for them all at once."""
    roots = {}
    for path, root in zero_addon.mission_files(game):
        try:
            roots.update({name: root for name in mission_scripts(path.read_bytes()) if name not in roots})
        except UNREADABLE:
            continue
    return roots


def map_problems(map_id, game, scripts, roots, add_on=False):
    """What keeps a map from preparing: its world, and each battle's armies
    and first sound bank (tools/extract.py's map and side steps fail
    without them). An add-on's map says what it needs by the name its
    scripts use, since a file nobody has has no one place to be."""
    from zero_sound import read_mission_audio

    def need(path, what, named):
        found = archive_problem(path, game, None if add_on else what)
        if found and add_on and found["code"] == "missing":
            return problem("missing", None, game, f"It needs {what}, {named}, which isn't in the add-on or the game.")
        return found

    found = [need(zero_addon.map_archive(map_id, game), "its world", f"{map_id[:3].upper()}/{map_id}.lvl")]
    battles = battles_of(map_id, scripts)
    if add_on and not battles:
        found.append(problem("missing", None, game, f"Its battles ({map_id} and an era letter) aren't in its mission.lvl."))
    for mission in sorted(battles):
        try:
            setup = mission_setup(battles[mission])
            banks = read_mission_audio(calls(load(battles[mission])))["banks"]
        except UNREADABLE as error:
            found.append(problem("unreadable", None, game, f"Its mission script {mission} can't be read ({error})."))
            continue
        for side in setup["sides"]:
            found.append(need(zero_addon.side_archive(side, game), "an army", f"SIDE/{side}.lvl"))
        if banks:
            named = banks[0].split(";", 1)[0].replace("\\", "/")
            found.append(need(sound_bank(banks[0], roots.get(mission), game), "its sound", named[3:] if named[:3].lower() == "dc:" else named))
    return tidy(found)


def language_problem(game):
    """None while the English files are there, else what this copy has
    instead (a copy in another language only)."""
    from zero_shell import archives
    folders = [Path(archives(game)[key]).parent for key in ("common", "shell")]
    if all(folder.is_dir() or spelled(folder) for folder in folders):
        return None  # there (or spelled otherwise, which is reported as misnamed)
    common = spelled(Path(game) / "Common")
    languages = sorted(entry.name for entry in common.iterdir() if entry.is_dir()) if common and common.is_dir() else []
    have = f" It has {', '.join(languages)} only." if languages else ""
    return problem("language", Path(game) / "Common" / LANGUAGE, game,
                   f"This copy has no English game files (Common/{LANGUAGE} and Shell/{LANGUAGE}).{have} "
                   "Battlefront Eternal reads the English ones, which the Steam and GOG copies include.")


def program_problem(game):
    """None, or why the maps' painted foliage can't be placed from this copy's
    Battlefront.exe (tools/zero_foliage.py reads its placement table)."""
    from zero_foliage import placement_table
    exe = Path(game).parents[1] / "Battlefront.exe"  # as tools/extract.py names it
    if not exe.is_file() or exe.stat().st_size == 0:
        found = archive_problem(exe, game) or problem("program", exe, game, f"{shown(exe, game)} is missing.")
        found["message"] += " The maps' grass and plants are placed from a table in it."
        return {**found, "code": "program"}
    try:
        placement_table(exe)
    except (ValueError, IndexError, struct.error, OSError):
        return problem("program", exe, game, f"{shown(exe, game)} isn't a version Battlefront Eternal knows (it may have been "
                                             "modified or replaced). The maps' grass and plants are placed from a table in it.")
    return None


def stock_problems(game=GAME):
    """What keeps the game itself from preparing: its folders, the shell's
    archives, the program, and every stock map's world, armies and sound.
    A folder that isn't there is the only problem reported."""
    from zero_shell import archives, ia_maps
    game = Path(game)
    for folder in (game.parents[1], game.parent, game):
        if not folder.is_dir():
            return [archive_problem(folder, game)]
    found = [language_problem(game)]
    for key, path in archives(game).items():
        if not (found[0] and key in ("common", "shell")):
            found.append(archive_problem(path, game))
    found.append(archive_problem(game / "SIDE", game))
    found.append(program_problem(game))
    try:
        scripts = mission_scripts((game / "mission.lvl").read_bytes())
        maps = ia_maps(archives(game)["shell"].read_bytes())
    except UNREADABLE:
        scripts, maps = {}, []  # reported above, as a missing or damaged archive
    roots = mission_roots(game)
    for map_id in maps:
        found += map_problems(map_id, game, scripts, roots)
    return tidy(found)


class only_add_ons:
    """Reads the installation with only these add-ons (BFE_SKIP_ADDONS naming
    the others) while it lasts."""

    def __init__(self, game, names):
        self.game, self.names = game, set(names)

    def __enter__(self):
        self.before = os.environ.get("BFE_SKIP_ADDONS")
        others = {add_on.name for add_on in zero_addon.add_ons(self.game, everything=True)} - self.names
        os.environ["BFE_SKIP_ADDONS"] = ",".join(sorted(zero_addon.skipped() | others))
        return self

    def __exit__(self, *_):
        if self.before is None:
            os.environ.pop("BFE_SKIP_ADDONS", None)
        else:
            os.environ["BFE_SKIP_ADDONS"] = self.before


def add_on_report(game=GAME, stock=()):
    """Each installed add-on, those a run leaves out too: its name, folder,
    the maps it lists (those the game doesn't have itself), and what keeps
    it out (problems: its own files can't be read) or one of its maps out
    (each map's problems)."""
    reports, listed = [], {map_id.lower() for map_id in stock}
    for add_on in zero_addon.add_ons(game, everything=True):
        report = {"name": add_on.name, "folder": add_on.folder.name, "maps": [], "problems": []}
        reports.append(report)
        # Its own scripts and text, which every run reads (the catalog lists
        # its maps, the battles' setup, the names), each an archive.
        own = [add_on.script] + [path for path in (zero_addon.child(add_on.root, name) for name in ("mission.lvl", "core.lvl")) if path]
        report["problems"] = [found for found in (archive_problem(path, game) for path in own) if found]
        if report["problems"]:
            continue
        reading = add_on.script
        try:
            entries = zero_addon.add_on_maps(add_on)
            for reading in own[1:]:
                if reading.name.lower() == "core.lvl":
                    read_localization(reading.read_bytes())
                else:
                    for chunk in mission_scripts(reading.read_bytes()).values():
                        mission_setup(chunk)
        except UNREADABLE as error:
            report["problems"].append(unreadable(reading, game, error))
            continue
        for entry in entries:
            map_id = entry["mapluafile"].lower()
            if map_id in listed:
                continue  # the game's own map, or listed already: the menu keeps the first
            listed.add(map_id)
            report["maps"].append({"id": map_id, "name": entry.get("showstr") or map_id, "problems": []})
    # The maps' own problems, and their names, with every readable add-on there.
    with only_add_ons(game, [report["name"] for report in reports if not report["problems"]]):
        from zero_spawn_screen import display_text
        try:
            scripts, roots = zero_addon.missions(game), mission_roots(game)
            locl = zero_addon.localization(game)
        except UNREADABLE:
            scripts, roots, locl = {}, {}, {}
        for report in reports:
            for entry in report["maps"]:
                entry["name"] = display_text(locl, entry["name"]) or entry["name"]
                try:
                    entry["problems"] = map_problems(entry["id"], game, scripts, roots, add_on=True)
                except UNREADABLE as error:
                    entry["problems"] = [problem("unreadable", None, game, f"Its files can't be read ({error}).")]
    return reports


def check(game=GAME):
    """The installation's check: ok when nothing keeps the game itself from
    preparing, with its problems, the stock maps' ids, and the add-ons
    (add_on_report). Reads only; writes nothing."""
    from zero_shell import archives, ia_maps
    game = Path(game)
    problems = stock_problems(game)
    try:
        stock = ia_maps(archives(game)["shell"].read_bytes())
    except UNREADABLE:
        stock = []
    try:
        add_ons = add_on_report(game, stock) if game.is_dir() else []
    except UNREADABLE as error:
        add_ons = []
        problems.append(problem("unreadable", game.parents[1] / "AddOn", game, f"The add-ons can't be read ({error})."))
    return {"ok": not problems, "game": str(install_root(game)), "problems": problems, "maps": stock, "addOns": add_ons}
