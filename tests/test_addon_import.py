"""Add-on maps (tools/zero_addon.py): a synthetic installation with an add-on
beside the game, as the Steam copy installs Jabba's Palace in AddOn/Tat3,
found, listed and read like the stock maps, whatever its name; and the
preparation running again for an install prepared before add-ons were read.
Nothing retail: the archives, Lua chunks and text are made here."""
import contextlib
import io
import json
import struct
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
import prepare  # noqa: E402
import zero_addon  # noqa: E402
import zero_lua  # noqa: E402
import zero_sound  # noqa: E402
from zero_hash import fnv1a  # noqa: E402
from zero_locl import localize  # noqa: E402
from zero_shell import catalog_table, map_catalog  # noqa: E402


def chunk(tag, body):
    return tag + struct.pack("<I", len(body)) + body + b"\0" * (-len(body) % 4)


def ucfb(*nodes):
    return chunk(b"ucfb", b"".join(nodes))


def op(name, u=None, s=None, a=0, b=0):
    """One Lua 4.0 instruction: an opcode with U (or S), or A and B."""
    code = zero_lua.OPS.index(name)
    if s is not None:
        u = s + zero_lua.MAXARG_S
    return code | (u << zero_lua.SIZE_OP) if u is not None else code | (b << zero_lua.SIZE_OP) | (a << zero_lua.POS_A)


def lua_string(text):
    raw = text.encode("latin1") + b"\0"
    return struct.pack("<I", len(raw)) + raw


def lua_chunk(strings, code):
    """A compiled Lua 4.0 chunk (the header zero_lua.load reads): one
    function with these string constants and instructions."""
    head = b"\x1bLua\x40\x01" + bytes([4, 4, 4, 32, 6, 9, 4]) + struct.pack("<f", 3.14159265e8)
    body = lua_string("=(none)") + struct.pack("<iiBi", 0, 0, 0, 16) + struct.pack("<ii", 0, 0)
    body += struct.pack("<i", len(strings)) + b"".join(lua_string(s) for s in strings)
    body += struct.pack("<ii", 0, 0) + struct.pack("<i", len(code)) + struct.pack(f"<{len(code)}I", *code)
    return head + body


def scripts(**named):
    """An archive of named scripts, as mission.lvl and addme.script hold them."""
    return ucfb(*(chunk(b"scr_", chunk(b"NAME", name.encode() + b"\0") + chunk(b"BODY", body)) for name, body in named.items()))


def calls_chunk(*calls):
    """Straight-line calls with literal arguments: ("SetTeamName", 1, "CIS")."""
    strings, code = [], []

    def constant(text):
        if text not in strings:
            strings.append(text)
        return strings.index(text)

    for name, *args in calls:
        code.append(op("GETGLOBAL", u=constant(name)))
        for arg in args:
            code.append(op("PUSHINT", s=arg) if isinstance(arg, int) else op("PUSHSTRING", u=constant(arg)))
        code.append(op("CALL", a=0, b=0))
    return lua_chunk(strings, code + [op("END")])


def addme(map_id, show, letters):
    """An addme script as the add-ons' are compiled: an entry appended to
    both of the shell's lists, then its missions registered."""
    strings = ["mapluafile", map_id, "showstr", show, *[f"side_{x}" for x in letters], "dnldable", "getn",
               "sp_missionselect_listbox_contents", "mp_missionselect_listbox_contents", "AddDownloadableContent",
               *[map_id + x for x in letters]]
    k = strings.index
    code = [op("CREATETABLE", u=2 + len(letters)), op("PUSHSTRING", u=k("mapluafile")), op("PUSHSTRING", u=k(map_id)),
            op("PUSHSTRING", u=k("showstr")), op("PUSHSTRING", u=k(show))]
    for x in letters:
        code += [op("PUSHSTRING", u=k(f"side_{x}")), op("PUSHINT", s=1)]
    code += [op("PUSHSTRING", u=k("dnldable")), op("PUSHINT", s=1), op("SETMAP", u=3 + len(letters))]
    for slot, name in ((1, "sp_missionselect_listbox_contents"), (2, "mp_missionselect_listbox_contents")):
        code += [op("GETGLOBAL", u=k("getn")), op("GETGLOBAL", u=k(name)), op("CALL", a=slot, b=1)]
        if slot == 2:
            code.append(op("SETLOCAL", u=1))
        code += [op("GETGLOBAL", u=k(name)), op("GETLOCAL", u=1), op("ADDI", s=1), op("GETLOCAL", u=0), op("SETTABLE", a=3, b=3)]
    for x in letters:
        code += [op("GETGLOBAL", u=k("AddDownloadableContent")), op("PUSHSTRING", u=k(map_id)),
                 op("PUSHSTRING", u=k(map_id + x)), op("PUSHINT", s=4), op("CALL", a=2, b=0)]
    return scripts(addme=lua_chunk(strings, code + [op("END")]))


def core(**texts):
    """core.lvl's English text: key -> text, keyed by hash."""
    body = b"".join(struct.pack("<IH", fnv1a(key.replace("__", ".")), 6 + 2 * len(text) + 2) + (text + "\0").encode("utf-16-le")
                    for key, text in texts.items())
    return ucfb(chunk(b"Locl", chunk(b"NAME", b"english\0") + chunk(b"BODY", body)))


def write(path, data=b"x"):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    return path


class Install:
    """GameData with the game's archives and two add-on folders: Abc (spelled
    as Tat3 spells its own, _lvl_pc and Side) and Junk, which has no
    addme.script and so isn't one."""

    def __init__(self, root):
        self.data = Path(root) / "GameData"
        self.game = self.data / "Data" / "_LVL_PC"
        write(self.game / "mission.lvl", scripts(
            geo1r=calls_chunk(("ReadDataFile", "SIDE\\rep.lvl"), ("ReadDataFile", "SIDE\\geo.lvl"), ("SetTeamName", 1, "Republic"))))
        write(self.game / "core.lvl", core(planets__geonosis__mapname1="GEONOSIS: SPIRE", level__geo1__cp1="ARENA"))
        write(self.game / "SIDE" / "rep.lvl")
        write(self.game / "GEO" / "geo1.lvl")
        write(self.game / "Sound" / "abc.lvl")
        self.addon = self.data / "AddOn" / "Abc"
        lvl = self.addon / "Data" / "_lvl_pc"
        write(self.addon / "addme.script", addme("ABC1", "ABC: TEST", "ri"))
        write(lvl / "mission.lvl", scripts(
            abc1r=calls_chunk(("ReadDataFile", "dc:sound\\abc.lvl;abc1cw"), ("ReadDataFile", "SIDE\\rep.lvl"),
                              ("ReadDataFile", "dc:SIDE\\abc.lvl"), ("SetTeamName", 1, "Republic"), ("SetTeamName", 2, "CIS"),
                              ("OpenAudioStream", "dc:sound\\abc.lvl", "abc1"), ("ReadDataFile", "dc:ABC\\abc1.lvl")),
            abc1i=calls_chunk(("ReadDataFile", "SIDE\\rep.lvl"), ("ReadDataFile", "dc:SIDE\\abc.lvl")),
            geo1r=calls_chunk(("SetTeamName", 1, "CIS"))))
        write(lvl / "core.lvl", core(level__abc1__cp1="THRONE ROOM", level__geo1__cp1="NOT THE GAME'S"))
        self.side = write(lvl / "Side" / "abc.lvl")
        self.world = write(lvl / "ABC" / "abc1.lvl")
        self.sound = write(lvl / "sound" / "abc.lvl")
        self.movies = write(lvl / "Movies" / "abc1.mvs")
        self.shell = write(lvl / "Shell" / "ENG" / "shell.lvl")
        self.root = lvl
        write(self.data / "AddOn" / "Junk" / "Data" / "_LVL_PC" / "XYZ" / "xyz1.lvl")


class DiscoveryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.install = Install(self.temp.name)
        self.game = self.install.game

    def tearDown(self):
        self.temp.cleanup()

    def test_an_add_on_is_a_folder_with_addme_and_data(self):
        found = zero_addon.add_ons(self.game)
        self.assertEqual([a.name for a in found], ["abc"])
        self.assertEqual(found[0].root, self.install.root, "Data/_lvl_pc matched without regard to case")
        self.assertEqual(prepare.add_on_names(self.game), ["abc"])
        self.assertEqual(zero_addon.add_ons(Path(self.temp.name) / "Nowhere" / "Data" / "_LVL_PC"), [])

    def test_archives_the_game_first_then_the_add_ons(self):
        self.assertEqual(zero_addon.map_archive("geo1", self.game), self.game / "GEO" / "geo1.lvl")
        self.assertEqual(zero_addon.map_archive("abc1", self.game), self.install.world)
        self.assertEqual(zero_addon.map_archive("xyz1", self.game), self.game / "XYZ" / "xyz1.lvl", "Junk isn't an add-on")
        self.assertEqual(zero_addon.side_archive("rep", self.game), self.game / "SIDE" / "rep.lvl")
        self.assertEqual(zero_addon.side_archive("abc", self.game), self.install.side, "SIDE matches Side")
        self.assertFalse(zero_addon.side_archive("zzz", self.game).exists())

    def test_dc_paths_are_the_add_ons_and_the_rest_the_games(self):
        root = zero_addon.mission_root("abc1i", self.game)
        self.assertEqual(root, self.install.root)
        self.assertIsNone(zero_addon.mission_root("geo1r", self.game))
        self.assertEqual(zero_addon.data_file("dc:sound\\abc.lvl;abc1cw", root, self.game), self.install.sound)
        self.assertEqual(zero_addon.data_file("sound\\abc.lvl", root, self.game), self.game / "Sound" / "abc.lvl")
        self.assertEqual(zero_addon.data_file("dc:SIDE\\abc.lvl", root, self.game), self.install.side)
        self.assertIsNone(zero_addon.data_file("dc:SIDE\\abc.lvl", None, self.game), "dc: outside an add-on")
        self.assertEqual(zero_sound.sound_file("dc:sound\\abc.lvl;abc1cw", root), self.install.sound)
        self.assertEqual(zero_addon.source_name(self.install.world, self.game), "AddOn/Abc/Data/_lvl_pc/ABC/abc1.lvl")
        self.assertEqual(zero_addon.source_name(self.game / "SIDE" / "rep.lvl", self.game), "SIDE/rep.lvl")

    def test_missions_and_text_merge_with_the_games_winning(self):
        found = zero_addon.missions(self.game)
        self.assertEqual(list(found), ["geo1r", "abc1r", "abc1i"])
        self.assertEqual(zero_lua.mission_setup(found["geo1r"])["teams"]["1"]["name"], "Republic", "a name the game's has stays the game's")
        self.assertEqual(zero_lua.mission_setup(found["abc1i"])["sides"], ["rep", "abc"], "dc:SIDE is a side")
        locl = zero_addon.localization(self.game)
        self.assertEqual(localize(locl, "level.abc1.cp1"), "THRONE ROOM")
        self.assertEqual(localize(locl, "level.geo1.cp1"), "ARENA")
        self.assertEqual(zero_addon.movie_archives(self.game), [(self.install.movies, self.install.shell)])

    def test_addme_appends_the_maps_to_both_lists(self):
        entry = {"mapluafile": "ABC1", "showstr": "ABC: TEST", "side_r": 1.0, "side_i": 1.0, "dnldable": 1.0, "addOn": "abc"}
        self.assertEqual(zero_addon.listed_maps(self.game), [entry])
        self.assertEqual(zero_addon.listed_maps(self.game, zero_addon.LISTS[1]), [entry])
        self.assertEqual(zero_lua.calls(zero_lua.load(zero_lua.mission_scripts(self.install.addon.joinpath("addme.script").read_bytes())["addme"]))[-1],
                         ("AddDownloadableContent", ["ABC1", "ABC1i", 4.0]))

    def test_the_catalog_lists_the_add_on_after_the_game_with_its_name_as_written(self):
        table = [{"mapluafile": "GEO1", "showstr": "planets.geonosis.mapname1", "side_r": 1.0}]
        table = catalog_table(table, zero_addon.listed_maps(self.game) + [{"mapluafile": "geo1", "showstr": "again"}])
        maps = map_catalog(table, zero_addon.missions(self.game), zero_addon.localization(self.game), {"abc1": "abc1r"},
                           {"abc1fly": {"file": "abc1fly.webm"}}, {})
        self.assertEqual([m["id"] for m in maps], ["geo1", "abc1"], "each map once, the add-on's last")
        abc1 = maps[1]
        self.assertEqual(abc1["name"], "ABC: TEST", "not a localization key: shown as written, as the shell does")
        self.assertTrue(abc1["extracted"])
        self.assertEqual(abc1["eras"]["cw"], {"teams": ["rep", "cis"], "attacker": 1, "mission": "abc1r", "extracted": True})
        self.assertEqual(abc1["eras"]["gcw"]["mission"], "abc1i")
        self.assertEqual(abc1["fly"], "movies/abc1fly.webm")
        self.assertIsNone(abc1["load"])

    def test_the_plan_finds_the_add_ons_armies_and_sound(self):
        game = prepare.GAME
        prepare.GAME = self.game
        try:
            self.assertEqual(prepare.mission_sides()["abc1"], ["rep", "abc"])
            maps = [{"id": "abc1", "eras": {"cw": {"teams": ["rep", "cis"]}}}]
            self.assertEqual(prepare.mission_sublevels(maps, prepare.mission_sides()), {"abc1": ["abc1cw"]})
        finally:
            prepare.GAME = game
        steps, _ = prepare.plan(8, maps=maps, locals_of={"abc1": ["rep", "abc"]}, sublevels={"abc1": ["abc1cw"]})
        keys = [step[0] for step in steps]
        self.assertIn("side:abc", keys)
        self.assertIn("map:abc1", keys)
        self.assertIn("audio:abc1cw", keys)


class WorldNamedOtherwiseTests(unittest.TestCase):
    """An add-on whose map id is not its world's name: a player's
    "georuins" failed with "no archive for georuins: expected
    GEO\\georuins.lvl", its battles reading GEO\\geo1.lvl."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.install = Install(self.temp.name)
        self.game = self.install.game
        self.lvl = self.install.data / "AddOn" / "Ruins" / "Data" / "_LVL_PC"
        write(self.install.data / "AddOn" / "Ruins" / "addme.script", addme("georuins", "GEONOSIS: RUINS", "rc"))

    def tearDown(self):
        self.temp.cleanup()

    def battles(self, world):
        write(self.lvl / "mission.lvl", scripts(
            georuinsr=calls_chunk(("ReadDataFile", "sound\\geo.lvl;geo1cw"), ("ReadDataFile", "SIDE\\rep.lvl"),
                                  ("ReadDataFile", world)),
            georuinsr_h=calls_chunk(("ReadDataFile", "GEO\\nothere.lvl"))))

    def test_its_own_copy_of_a_world_named_otherwise(self):
        self.battles("dc:GEO\\geo1.lvl")
        own = write(self.lvl / "GEO" / "geo1.lvl", b"ruins")
        self.assertEqual(zero_addon.map_archive("georuins", self.game), own)
        self.assertEqual(zero_addon.map_archive("geo1", self.game), self.game / "GEO" / "geo1.lvl", "the stock map keeps its own")

    def test_the_games_world_read_under_another_name(self):
        self.battles("GEO\\geo1.lvl")
        self.assertEqual(zero_addon.map_archive("georuins", self.game), self.game / "GEO" / "geo1.lvl")

    def test_no_world_there_is_still_the_message_path(self):
        self.battles("dc:GEO\\missing.lvl")
        self.assertEqual(zero_addon.map_archive("georuins", self.game), self.game / "GEO" / "georuins.lvl")
        self.assertIsNone(zero_addon.world_read(zero_lua.mission_scripts(write(Path(self.temp.name) / "x.lvl", scripts(
            x=calls_chunk(("ReadDataFile", "SIDE\\rep.lvl"), ("ReadDataFile", "sound\\geo.lvl;geo1cw")))).read_bytes())["x"]))


class MissionAudioTests(unittest.TestCase):
    def test_an_add_ons_sound_bank_and_streams(self):
        m = zero_sound.read_mission_audio([("ReadDataFile", ["dc:sound\\tat.lvl;tat3gcw"]), ("ReadDataFile", ["dc:SIDE\\gam.lvl"]),
                                           ("OpenAudioStream", ["dc:sound\\tat.lvl", "tat3"]), ("OpenAudioStream", ["sound\\gcw.lvl", "gcw_vo"])])
        self.assertEqual(m["banks"], ["dc:sound\\tat.lvl;tat3gcw"])
        self.assertEqual(m["streams"], [["dc:sound\\tat.lvl", "tat3"], ["sound\\gcw.lvl", "gcw_vo"]])


class GlobalTablesTests(unittest.TestCase):
    def test_getn_only_when_given(self):
        chunk = zero_lua.load(zero_lua.mission_scripts(addme("ABC1", "ABC", "r"))["addme"])
        self.assertEqual(zero_lua.global_tables(chunk), {}, "no lists to append to, no getn: nothing")
        lists = zero_lua.global_tables(chunk, env={"sp_missionselect_listbox_contents": {1.0: {"mapluafile": "GEO1"}}},
                                       builtins={"getn": zero_lua.getn})
        self.assertEqual([item["mapluafile"] for item in lists["sp_missionselect_listbox_contents"]], ["GEO1", "ABC1"])
        self.assertEqual(zero_lua.getn({1.0: "a", 2.0: "b", 4.0: "d"}), 2.0)


def soft_model(name, records, bmap):
    """A model of one triangle with a soft skin (SKIN type 3: two weights
    and three BMAP slots a vertex), its vertices in the model's space."""
    positions = [(0.0, 0.0, 0.0), (0.0, 1.0, 0.0), (1.0, 0.0, 0.0)]
    flags = 0b10 | 0b100000 | 0b1000000000  # POS, NORMAL, UV
    vertices = b"".join(struct.pack("<3f3f2f", *pos, 0.0, 1.0, 0.0, 0.0, 0.0) for pos in positions)
    segment = (chunk(b"INFO", struct.pack("<III", 4, 3, 1)) + chunk(b"TNAM", struct.pack("<I", 0) + name.encode() + b"\0")
               + chunk(b"IBUF", struct.pack("<I3H", 3, 0, 1, 2)) + chunk(b"VBUF", struct.pack("<III", 3, 32, flags) + vertices)
               + chunk(b"SKIN", struct.pack("<II", 3, 3) + bytes(b for record in records for b in record))
               + chunk(b"BMAP", struct.pack("<I", len(bmap)) + bytes(bmap)))
    info = struct.pack("<3I12f", 0, 0, 0, *([-1.0] * 3 + [1.0] * 3 + [0.0] * 3 + [1.0] * 3))
    return chunk(b"modl", chunk(b"NAME", name.encode() + b"\0") + chunk(b"INFO", info) + chunk(b"segm", segment))


class SoftSkinTests(unittest.TestCase):
    def test_each_vertex_follows_its_heaviest_bone_where_it_is(self):
        """Jabba's Palace's Gamorrean guard is soft-skinned. Read as a hard
        skin (one bone byte a vertex), its body came apart."""
        import array
        from extract import write_actor
        from zero_model import parse_model
        moved = [1.0, 0, 0, 0, 0, 1.0, 0, 0, 0, 0, 1.0, 0, 5.0, 5.0, 5.0, 1.0]  # every bone 5 m off the origin
        skeleton = {"names": ["root", "bone_a", "bone_b"], "parents": [None, "root", "root"], "local": [moved] * 3, "model": [moved] * 3}
        # Weights (255, 0, 0) on slot 1; (100, 50, 105) heaviest on slot 2; (0, 200, 55) on slot 1.
        data = soft_model("gam", [(255, 0, 1, 0, 0), (100, 50, 0, 1, 2), (0, 200, 0, 1, 2)], [2, 1, 0])
        name, parts = parse_model(data, 8, len(data), {"gam": skeleton})
        self.assertEqual(name, "gam")
        self.assertEqual(parts[0]["skin"], [1, 2, 1], "the heaviest of each vertex's three slots")
        self.assertEqual(parts[0]["joints"], [1, 0, 1], "through BMAP")
        self.assertEqual(parts[0]["positions"][1], (0.0, 1.0, 0.0), "where it is, not posed again")
        with tempfile.TemporaryDirectory() as out, contextlib.redirect_stdout(io.StringIO()):
            self.assertIsNotNone(write_actor({"gam": parts}, {"gam": skeleton}, "gam", Path(out)))
            floats = array.array("f", (Path(out) / "gam.bin").read_bytes())
            self.assertEqual([floats[i * 9 + 8] for i in range(3)], [1.0, 0.0, 1.0])


class PreparedStateTests(unittest.TestCase):
    def test_add_ons_installed_since_run_what_lists_them_again(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / ".prepare.json"
            done = ["catalog", "side:rep", "common", "shell", "hud", "movies", "map:geo1", "audio:geo1cw", "catalog-update",
                    "environment", "textures"]
            path.write_text(json.dumps({"version": prepare.STATE_VERSION, "game": str(prepare.GAME), "done": done,
                                        "mapImportVersion": prepare.MAP_IMPORT_VERSION, "muzzleVersion": prepare.SIDE_EXPORT_VERSION, "commonVersion": prepare.COMMON_VERSION, "complete": True}))
            state = prepare.State(path, installed=["tat3"])
            self.assertEqual(state.data["done"], ["side:rep", "shell", "hud", "map:geo1", "audio:geo1cw", "environment", "textures"])
            self.assertEqual(state.data["addOns"], ["tat3"])
            for key in ("catalog", "common", "movies", "catalog-update"):
                state.mark(key)
            again = prepare.State(path, installed=["tat3"])
            self.assertTrue(all(again.done(key) for key in done), "the same add-ons: nothing runs again")
            removed = prepare.State(path, installed=[])
            self.assertFalse(removed.done("catalog"), "an add-on removed: the catalog drops its maps")
            self.assertEqual(removed.data["addOns"], [])

    def test_no_add_ons_then_or_now_changes_nothing(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / ".prepare.json"
            path.write_text(json.dumps({"version": prepare.STATE_VERSION, "game": str(prepare.GAME), "done": ["catalog", "common"],
                                        "mapImportVersion": prepare.MAP_IMPORT_VERSION, "muzzleVersion": prepare.SIDE_EXPORT_VERSION, "commonVersion": prepare.COMMON_VERSION}))
            self.assertEqual(prepare.State(path, installed=[]).data["done"], ["catalog", "common"])


if __name__ == "__main__":
    unittest.main()
