"""Per-game profiles for the preparation tools (Star Wars: Battlefront 2004
and Star Wars: Battlefront II 2005).

The client sets BFE_FAMILY ("swbf1" by default, or "swbf2"), BFE_GAME_DIR
(that game's install folder) and BFE_CONTENT_DIR (already the family's own
content folder). Every tool asks this module instead of carrying its own
copy of the install layout, shared archive set, language folder rule,
executable name, era/side tables, add-on layout or script reader choice.
"""

from dataclasses import dataclass
import json
import os
from pathlib import Path
import re
import subprocess


SWBF1 = "swbf1"
SWBF2 = "swbf2"
FAMILIES = (SWBF1, SWBF2)


@dataclass(frozen=True)
class FamilyProfile:
    id: str
    title: str
    data_subpath: tuple
    default_install_subpath: tuple
    program_name: str
    has_foliage_table: bool
    language_folders: bool
    side_dir: str
    sound_dir: str
    load_dir: str
    movies_dir: str
    fpm_dir: str
    planet_dir_upper: bool
    addon_dir: str
    addon_data_subpath: tuple
    addon_script: str
    load_archive: str
    extra_archives: tuple
    shared_archive_keys: tuple
    era_teams: dict
    era_sides: dict
    side_eras: dict
    mission_eras: dict
    default_era: str
    side_letters: tuple
    faction_sides: dict
    side_names: dict
    lua_version: str
    script_reader: str

    def planet_folder(self, map_id):
        prefix = map_id[:3]
        return prefix.upper() if self.planet_dir_upper else prefix.lower()

    def map_relpath(self, map_id):
        return f"{self.planet_folder(map_id)}/{map_id}.lvl"

    def side_relpath(self, side):
        return f"{self.side_dir}/{side}.lvl"

    def program_path(self, game):
        return Path(game).parents[1] / self.program_name

    def mission_pattern(self, map_id):
        if self.id == SWBF2:
            return re.escape(map_id) + r"[cg]_[a-z0-9_]+"
        return re.escape(map_id) + r"[a-z](_h)?"

    def archives(self, game):
        """The archives the front end and preparation read under game."""
        import zero_language
        game = Path(game)
        found = {
            "common": zero_language.common_archive(game, family=self.id),
            "core": game / "core.lvl",
            "shell": zero_language.shell_archive(game, family=self.id),
            "load": game / self.load_archive,
            "sound": game / self.sound_dir / "shell.lvl",
            "bank": game / self.sound_dir / "common.bnk",
            "movies": game / self.movies_dir / "shell.mvs",
            "mission": game / "mission.lvl",
        }
        for key, rel in self.extra_archives:
            found[key] = game / rel
        return found

    def shared_archive_paths(self, game):
        """Archives every map and side can draw models, classes and textures from."""
        found = self.archives(game)
        return [found[key] for key in self.shared_archive_keys]


_SWBF1_SIDE_NAMES = {
    "rep": "Republic", "cis": "CIS", "all": "Rebel Alliance", "imp": "Galactic Empire",
    "geo": "Geonosians", "ewk": "Ewoks", "wok": "Wookiees", "gun": "Gungans", "des": "Tusken Raiders",
    "gar": "Garrison", "jaw": "Jawas", "gam": "Gamorreans",
}

_SWBF2_SIDE_NAMES = {
    **_SWBF1_SIDE_NAMES,
    "jed": "Jedi", "tur": "Turrets", "snw": "Wampas",
}


PROFILES = {
    SWBF1: FamilyProfile(
        id=SWBF1,
        title="Star Wars Battlefront",
        data_subpath=("Data", "_LVL_PC"),
        default_install_subpath=("data", "game", "install"),
        program_name="Battlefront.exe",
        has_foliage_table=True,
        language_folders=True,
        side_dir="SIDE",
        sound_dir="Sound",
        load_dir="Load",
        movies_dir="Movies",
        fpm_dir="FPM",
        planet_dir_upper=True,
        addon_dir="AddOn",
        addon_data_subpath=("Data", "_LVL_PC"),
        addon_script="addme.script",
        load_archive="Load/load.lvl",
        extra_archives=(),
        shared_archive_keys=("common", "core"),
        era_teams={"cw": ["rep", "cis"], "gcw": ["all", "imp"]},
        era_sides={"cw": ("rep", "cis"), "gcw": ("all", "imp")},
        side_eras={"rep": "cw", "cis": "cw", "all": "gcw", "imp": "gcw"},
        mission_eras={"r": "cw", "c": "cw", "a": "gcw", "i": "gcw"},
        default_era="cw",
        side_letters=(("r", "cw", "rep"), ("c", "cw", "cis"), ("a", "gcw", "all"), ("i", "gcw", "imp")),
        faction_sides={1: "all", 2: "imp", 3: "rep", 4: "cis"},
        side_names=_SWBF1_SIDE_NAMES,
        lua_version="4.0",
        script_reader="zero_lua",
    ),
    SWBF2: FamilyProfile(
        id=SWBF2,
        title="Star Wars Battlefront II",
        data_subpath=("data", "_lvl_pc"),
        default_install_subpath=("data", "game", "swbf2", "install"),
        program_name="BattlefrontII.exe",
        has_foliage_table=False,
        language_folders=False,
        side_dir="side",
        sound_dir="sound",
        load_dir="load",
        movies_dir="movies",
        fpm_dir="fpm",
        planet_dir_upper=False,
        addon_dir="addon",
        addon_data_subpath=("data", "_lvl_pc"),
        addon_script="addme.script",
        load_archive="load/common.lvl",
        extra_archives=(
            ("ingame", "ingame.lvl"),
            ("inshell", "inshell.lvl"),
            ("global_sound", "sound/global.lvl"),
        ),
        shared_archive_keys=("common", "core", "ingame"),
        era_teams={"cw": ["rep", "cis"], "gcw": ["all", "imp"]},
        era_sides={"cw": ("rep", "cis"), "gcw": ("all", "imp")},
        side_eras={"rep": "cw", "cis": "cw", "all": "gcw", "imp": "gcw"},
        mission_eras={"c": "cw", "g": "gcw"},
        default_era="cw",
        side_letters=(("c", "cw", "rep"), ("g", "gcw", "all")),
        faction_sides={1: "all", 2: "imp", 3: "rep", 4: "cis"},
        side_names=_SWBF2_SIDE_NAMES,
        lua_version="5.0",
        script_reader="bfe_script_tool",
    ),
}


def current():
    """Active game family from BFE_FAMILY (swbf1 by default)."""
    raw = os.environ.get("BFE_FAMILY", SWBF1).strip().lower()
    return raw if raw in PROFILES else SWBF1


def profile(family=None):
    """FamilyProfile for family (or BFE_FAMILY when omitted)."""
    name = current() if family is None else str(family).strip().lower()
    if name not in PROFILES:
        raise ValueError(f"Unknown game family {name!r}; expected one of {', '.join(FAMILIES)}")
    return PROFILES[name]


def is_swbf2(family=None):
    return profile(family).id == SWBF2


def script_tool():
    """Path to bfe-script-tool from BFE_SCRIPT_TOOL, or None when unset."""
    raw = os.environ.get("BFE_SCRIPT_TOOL", "").strip()
    return Path(raw) if raw else None


def preparation_manifest(payload, script_name=None):
    """Read the recorder's ordered calls and preserve its schema-specific diagnostics."""
    if not isinstance(payload, dict):
        raise ValueError("Mission recorder manifest must be a JSON object")
    version = payload.get("schema")
    if type(version) is not int or version not in (1, 2, 3, 4):
        raise ValueError(f"Unsupported mission recorder manifest schema {version!r} (expected 1 through 4)")
    name = payload.get("script")
    if not isinstance(name, str) or not name or (script_name and name.lower() != script_name.lower()):
        raise ValueError(f"Invalid mission recorder script {name!r} (requested {script_name!r})")
    if not isinstance(payload.get("calls"), list):
        raise ValueError(f"Mission recorder manifest for {name} has no ordered calls array")
    if version >= 2 and (not isinstance(payload.get("profile"), str) or not isinstance(payload.get("unresolved_queries"), list)):
        raise ValueError(f"Mission recorder schema {version} for {name} requires profile and unresolved_queries")
    if version >= 3 and not isinstance(payload.get("probed_globals"), list):
        raise ValueError(f"Mission recorder schema {version} for {name} requires probed_globals")
    if version >= 4 and not isinstance(payload.get("skipped_scripts"), list):
        raise ValueError(f"Mission recorder schema {version} for {name} requires skipped_scripts")

    def scalar(arg):
        # Archive strings use a reversible byte-to-character mapping. Keep
        # exceptional Lua values and typed table keys intact in the trace.
        if isinstance(arg, dict) and arg.get("$bfe") == "bytes":
            try:
                return bytes.fromhex(arg["hex"]).decode("latin1")
            except (KeyError, TypeError, ValueError) as error:
                raise ValueError(f"Invalid byte string in mission recorder manifest for {name}") from error
        if arg == {"$bfe": "number", "special": "-0"}:
            return -0.0
        return arg

    calls = []
    for call in payload["calls"]:
        if not isinstance(call, dict) or not isinstance(call.get("name"), str) or not isinstance(call.get("args"), list):
            raise ValueError(f"Malformed mission recorder call for {name}")
        if version >= 2:
            args = [scalar(arg) for arg in call["args"]]
            if args != call["args"]:
                call = {**call, "args": args, "wireArgs": call["args"]}
        if call["name"] == "ReadDataFile" and (not call["args"] or not isinstance(call["args"][0], str)):
            raise ValueError(f"Malformed ReadDataFile path in mission recorder manifest for {name}")
        calls.append(call)
    return {**payload, "calls": calls}


RECORDER_TIMEOUT_SECONDS = 60


def read_mission_manifest(mission_archive, script_name, tool=None):
    """Run `bfe-script-tool manifest <mission.lvl> <script>` and return its
    parsed JSON manifest. Raises FileNotFoundError when BFE_SCRIPT_TOOL is
    not set or missing, and RuntimeError when the tool fails."""
    exe = Path(tool) if tool is not None else script_tool()
    if exe is None or not exe.exists():
        raise FileNotFoundError(
            "BFE_SCRIPT_TOOL is not set or does not exist; reading 2005 mission scripts requires bfe-script-tool"
        )
    try:
        run = subprocess.run(
            [str(exe), "manifest", str(mission_archive), str(script_name)],
            capture_output=True, text=True, encoding="utf-8", errors="replace",
            timeout=RECORDER_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(f"Mission recorder timed out after {RECORDER_TIMEOUT_SECONDS} seconds for {script_name}") from error
    if run.returncode != 0:
        detail = (run.stderr or run.stdout or "").strip()
        raise RuntimeError(f"bfe-script-tool manifest failed for {script_name} (rc={run.returncode}): {detail}")
    return preparation_manifest(json.loads(run.stdout), script_name)
