#!/usr/bin/env python3
"""The scoreboard's data: the art and text the original's end-of-match
table (and Tab's table, which this port builds from it), its DETAILS and
AWARDS, and the stat popup draw. docs/scoreboard-assets.md has the schema.

    python3 tools/zero_scoreboard.py

tools/extract.py writes all of it as it extracts common. This command adds it
to an extraction made before, and rewrites nothing else: common.json gains
the list textures, the table's strings and their rasterHud entries. The
textures it adds get KTX2 copies (tools/ktx2.py) and, with Pillow, 4x WebP
atlases (tools/upscale_hud.py), as do the stat icons already there.
"""

import json
import sys
from pathlib import Path

from zero_spawn_screen import core_localization, display_text, finish_common
from zero_locl import localize

ROOT = Path(__file__).resolve().parents[1]
from zero_paths import PUBLIC  # noqa: E402

# The table's art in common.lvl: the dimming behind it (opaque_black, 16x16
# black at alpha 119), the selection's and the pointer's row frames
# (listbox_cursor, listbox_hilight, 9-sliced), the scroll arrow
# (small_arrow, turned to point up or down), and DETAILS' tab
# (backbutton_left, _mid, _right). The window (border_3_pieces), the intro's
# block (cursor) and the pointer (mousecursor) come with the spawn screen.
SCOREBOARD_TEXTURES = (
    "opaque_black", "listbox_cursor", "listbox_hilight", "small_arrow",
    "backbutton_left", "backbutton_mid", "backbutton_right",
)
# Already in common for the stat popup (extract.py HUD_TEXTURES); the
# headers use them too.
STAT_ICONS = ("stats_kills", "stats_deaths", "stats_cpp")
# Art tools/upscale_hud.py enlarges besides its name rule: all of the
# above (the stat icons aren't hud*, *_icon or font pages).
SCOREBOARD_RASTER = SCOREBOARD_TEXTURES + STAT_ICONS
# The table's text: its columns, its buttons, Tab's control's name.
TABLE_STRINGS = (
    "ifs.Stats.kills", "ifs.stats.deaths", "ifs.stats.commandpostpoints",
    "ifs.stats.done", "ifs.Stats.details", "ifs.Stats.awards",
    "ifs.controls.playerlist", "game.pause.playerlist",
)
# DETAILS' title and rows (ifs_personalstats), in the screen's order.
DETAILS_STRINGS = (
    "ifs.stats.personalstatstitle", "ifs.stats.headshots", "ifs.stats.favclass", "ifs.stats.favweapon",
    "ifs.stats.favvehicle", "ifs.stats.livingstreak", "ifs.stats.narrowescapes", "ifs.stats.vehiclesdestroyed",
    "ifs.stats.nemesis", "ifs.stats.bait",
)
# AWARDS' title and awards (ifs_awardstats), in the screen's order. The
# original game asks for them by hash; BANTHA FODDER's key is ifs.stats.meat.
AWARDS_STRINGS = (
    "ifs.stats.awardstatstitle", "ifs.stats.tankbuster", "ifs.stats.killingspree", "ifs.stats.camper",
    "ifs.stats.survivalist", "ifs.stats.traitor", "ifs.stats.publicenemy", "ifs.stats.meat", "ifs.stats.deadeye",
)
SCOREBOARD_STRINGS = TABLE_STRINGS + DETAILS_STRINGS + AWARDS_STRINGS


def scoreboard_strings(locl):
    """The table's strings that the language has, by key."""
    return {key: display_text(locl, key) for key in SCOREBOARD_STRINGS if localize(locl, key) is not None}


def refresh_common(locl, public=PUBLIC):
    """Add the table's textures, strings and rasterHud entries to an
    extracted common.json, writing only the new PNGs."""
    from extract import collect_textures, shared_archives, write_textures
    out = public / "common"
    path = out / "common.json"
    if not path.exists():
        print("common: no common.json; tools/extract.py writes it", flush=True)
        return None
    common = json.loads(path.read_text())
    textures = {}
    for archive in shared_archives():
        collect_textures(archive, textures)
    wanted = set(SCOREBOARD_TEXTURES) | set(STAT_ICONS)
    added = write_textures(textures, wanted - set(common["textures"]), out / "textures")
    common["textures"] = dict(sorted({**common["textures"], **added}.items()))
    strings = scoreboard_strings(locl)
    common["strings"] = {**common.get("strings", {}), **strings}
    raster = list(common.get("rasterHud", []))
    raster += [name for name in SCOREBOARD_RASTER if name in common["textures"] and name not in raster]
    common["rasterHud"] = raster
    path.write_text(json.dumps(common))
    print(f"common: textures {sorted(added)} strings {len(strings)} rasterHud {common['rasterHud']}", flush=True)
    return common


def main():
    locl = core_localization()
    if refresh_common(locl) is not None:
        finish_common()
    return 0


if __name__ == "__main__":
    sys.exit(main())
