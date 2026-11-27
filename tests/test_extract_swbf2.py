"""Synthetic unit tests for 2005 map/side/common extraction (tools/extract_swbf2.py)
and CLI dispatch through tools/extract.py with BFE_FAMILY=swbf2. Nothing retail."""

import json
import math
import os
import struct
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "tools"))
sys.path.insert(0, str(HERE))
from test_addon_import import chunk, ucfb, write  # noqa: E402
from test_language import text as locl_archive  # noqa: E402
from test_swbf2_readers import cfg_data, cfg_scop, lvl  # noqa: E402
from zero_hash import fnv1a  # noqa: E402
import zero_model  # noqa: E402
import zero_swbf2  # noqa: E402


def _make_tex(name):
    # 1x1 A8R8G8B8 texture
    fmt_info = struct.pack("<IHHHHI", 21, 1, 1, 1, 1, 1)
    lvl0 = chunk(b"LVL_", chunk(b"INFO", struct.pack("<II", 0, 4)) + chunk(b"BODY", bytes([255, 128, 64, 255])))
    fmt = chunk(b"FMT_", chunk(b"INFO", fmt_info) + chunk(b"FACE", lvl0))
    return chunk(b"tex_", chunk(b"NAME", name.encode() + b"\x00") + chunk(b"INFO", struct.pack("<II", 1, 21)) + fmt)


def _make_modl_swbf2(name, tex_name, verts=None, attachment=None, sphere=None):
    verts = verts or [(0.0, 0.0, 0.0), (2.0, 0.0, 0.0), (0.0, 2.0, 0.0)]
    info72 = struct.pack("<4i6f6f2i", 1, 1, 1, 0, 0, 0, 0, 2, 2, 0, 0, 0, 0, 2, 2, 0, 0, 1)
    mtrl = struct.pack("<6I", zero_model.SWBF2_NORMAL, 0xFFFFFFFF, 0xFFFFFFFF, 0, 0, 0) + b"\x00"
    # VBUF: flags = POS | NORMAL | UV = 0x222, stride = 32 bytes (3f pos, 3f nrm, 2f uv)
    extra = zero_model.TANGENTS | zero_model.STATIC_LIT | zero_model.COLOR if attachment else 0
    vbuf_body = bytearray(struct.pack("<III", len(verts), 64 if attachment else 32, 0x222 | extra))
    for x, y, z in verts:
        vbuf_body.extend(struct.pack("<6f", x, y, z, 0.0, 1.0, 0.0))
        if attachment:
            vbuf_body.extend(struct.pack("<6fII", 1, 0, 0, 0, 0, 1, 0xFFFF0000, 0xFF00FF00))
        vbuf_body.extend(struct.pack("<2f", 0.0, 0.0))
    ibuf_body = struct.pack("<I3H", 3, 0, 1, 2)
    segm = chunk(
        b"segm",
        chunk(b"INFO", struct.pack("<III", zero_model.TRI_LIST, len(verts), 1))
        + chunk(b"MTRL", mtrl)
        + chunk(b"RTYP", b"Normal\x00")
        + (chunk(b"BNAM", attachment.encode() + b"\0") if attachment else b"")
        + chunk(b"TNAM", struct.pack("<I", 0) + tex_name.encode() + b"\x00")
        + chunk(b"VBUF", bytes(vbuf_body))
        + chunk(b"IBUF", ibuf_body),
    )
    sphr = chunk(b"SPHR", struct.pack("<4f", *sphere)) if sphere else b""
    return chunk(b"modl", chunk(b"NAME", name.encode() + b"\x00") + chunk(b"INFO", info72) + sphr + segm)


def _make_entc(cname, base, props):
    body = chunk(b"TYPE", cname.encode() + b"\x00") + chunk(b"BASE", base.encode() + b"\x00")
    for k, v in props:
        body += chunk(b"PROP", struct.pack("<I", fnv1a(k)) + v.encode() + b"\x00")
    return chunk(b"entc", body)


def _make_wpnc(cname, base, props):
    body = chunk(b"TYPE", cname.encode() + b"\x00") + chunk(b"BASE", base.encode() + b"\x00")
    for k, v in props:
        body += chunk(b"PROP", struct.pack("<I", fnv1a(k)) + v.encode() + b"\x00")
    return chunk(b"wpnc", body)


def _make_inst(cname, iname, pos, props=()):
    xfrm = struct.pack("<12f", 1, 0, 0, 0, 1, 0, 0, 0, 1, pos[0], pos[1], pos[2])
    info = chunk(b"INFO", chunk(b"TYPE", cname.encode() + b"\x00") + chunk(b"NAME", iname.encode() + b"\x00") + chunk(b"XFRM", xfrm))
    body = info
    for k, v in props:
        body += chunk(b"PROP", struct.pack("<I", fnv1a(k)) + v.encode() + b"\x00")
    return chunk(b"inst", body)


def _make_regn(rtype, rname, pos, size):
    xfrm = struct.pack("<12f", 1, 0, 0, 0, 1, 0, 0, 0, 1, pos[0], pos[1], pos[2])
    info = chunk(
        b"INFO",
        chunk(b"TYPE", rtype.encode() + b"\x00")
        + chunk(b"NAME", rname.encode() + b"\x00")
        + chunk(b"XFRM", xfrm)
        + chunk(b"SIZE", struct.pack("<3f", *size)),
    )
    return chunk(b"regn", info)


class ExtractSwbf2Tests(unittest.TestCase):
    def test_navigation_records_keep_their_world_layer_and_source_order(self):
        import extract
        import extract_swbf2

        pose = [0, 0, -1, 0, 1, 0, 1, 0, 0, -7.25, 3.5, 11.75]
        name = chunk(b"NAME", b"reed\0")
        xfrm = chunk(b"XFRM", struct.pack("<12f", *pose))

        def prop(key, value):
            return chunk(b"PROP", struct.pack("<I", key) + value + b"\0")

        def hint(kind, *properties):
            info = chunk(b"TYPE", kind + b"\0") + name + xfrm
            return chunk(b"Hint", chunk(b"INFO", info) + b"".join(properties))

        def barrier(flags):
            info = name + xfrm + chunk(b"SIZE", struct.pack("<3f", 2.5, 6, 0.75))
            info += chunk(b"FLAG", struct.pack("<I", flags))
            return chunk(b"BARR", prop(fnv1a("Layer"), b"raffia") + chunk(b"INFO", info))

        def world(label, *records):
            return chunk(b"wrld", chunk(b"NAME", label.encode() + b"\0") + b"".join(records))

        radius = fnv1a("Radius")
        repeated = hint(b"7", prop(radius, b" 02.50"), prop(0xCAFEBABE, b"MiXeD"),
                        prop(radius, b"0.750"))
        archive = ucfb(world("loom", hint(b"unfamiliar"), barrier(0x80000020)),
                       world("loom_east", repeated),
                       lvl("loom_reeds", world("loom_reeds", barrier(1), hint(b"8"), barrier(0))),
                       lvl("loom_yard", world("loom_yard")))
        with tempfile.TemporaryDirectory() as directory:
            game = Path(directory)
            output = game / "prepared"
            path = game / "loom.lvl"
            path.write_bytes(archive)
            with (mock.patch.dict(os.environ, {"BFE_FAMILY": "swbf2"}),
                  mock.patch.object(extract, "map_archive", return_value=path),
                  mock.patch.object(extract_swbf2, "GAME", game),
                  mock.patch.object(extract_swbf2, "PUBLIC", output)):
                extract_swbf2.extract_map_swbf2("loom", [])
            prepared = json.loads((output / "maps/loom/map.json").read_bytes())

        common = {"name": "reed", "xfrm": pose}
        fence = {**common, "size": [2.5, 6, 0.75], "enabled": True,
                 "properties": [[f"{fnv1a('Layer'):08x}", "Layer", "raffia"]]}
        self.assertEqual(prepared["barriers"], [{**fence, "sourceIndex": 0, "flags": 0x80000020}])
        self.assertEqual(prepared["hints"], [
            {**common, "sourceIndex": 0, "type": "unfamiliar", "properties": []},
            {**common, "sourceIndex": 1, "type": "7", "properties": [
                [f"{radius:08x}", "Radius", " 02.50"], ["cafebabe", None, "MiXeD"],
                [f"{radius:08x}", "Radius", "0.750"]]}])
        reeds = prepared["layers"]["loom_reeds"]
        self.assertEqual(reeds["barriers"], [{**fence, "sourceIndex": 0, "flags": 1},
                                             {**fence, "sourceIndex": 1, "flags": 0}])
        self.assertEqual(reeds["hints"], [{**common, "sourceIndex": 0, "type": "8", "properties": []}])
        yard = prepared["layers"]["loom_yard"]
        self.assertEqual((yard["barriers"], yard["hints"]), ([], []))

    def test_sky_lookup_names_available_hashes_from_install_folders(self):
        import extract_swbf2
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(extract_swbf2, "GAME", Path(tmp)):
            (Path(tmp) / "volcanic").mkdir()
            write(Path(tmp) / "spa/spa_sky.lvl", ucfb(lvl("volcanic"), lvl("second")))
            _, _, source = extract_swbf2._load_sky_subarchive("spa9", "SECOND")
            self.assertEqual(source, "spa/spa_sky.lvl;second")
            with self.assertRaisesRegex(ValueError, f"{fnv1a('absent'):08x}.*volcanic"):
                extract_swbf2._load_sky_subarchive("spa9", "absent")

    def test_combo_state_timing_uses_this_hero_and_lists_missing_animations(self):
        from extract_swbf2 import _combo_bindings
        from zero_hash import crc
        def clip(name, frames):
            return {"crc": f"{crc(name):08x}", "frames": frames}
        common = {"clips": [clip("human_sabre_fall_full", 5), clip("human_sabre_parry", 2)]}
        weapon = {"properties": [("ComboAnimationBank", "human_sabre melee example")]}
        tree = {"animations": [{"values": ["fall_upper"]}], "offhandAnimations": [], "throwAnimations": [],
                "states": [{"name": "AIR", "children": [
                    {"key": "Animation", "values": ["fall"], "children": [{"key": "Loop", "values": ["FinalFrame"]}]},
                    {"key": "Deflect", "children": [{"key": "DeflectAnimation", "values": ["parry", "Forward"]}]},
                    {"key": "Animation", "values": ["unavailable"], "children": [{"key": "Loop"}]}]}]}
        for hero, frames in (("first", 9), ("second", 21)):
            banks = {"human_sabre": common, hero: {"clips": [clip(hero + "_sabre_fall_full", frames)]}}
            b, = _combo_bindings(weapon, {f"{fnv1a('example'):08x}": tree}, banks, 24, hero)
            self.assertEqual([m["bank"] for m in b["clips"]["fall_upper"]], ["human_sabre", hero])
            self.assertEqual(b["clipTiming"]["fall"], {**b["clips"]["fall"][1], "frames": frames, "fps": 24})
            self.assertEqual(b["clipTiming"]["parry"]["bank"], "human_sabre")
            self.assertEqual([(r["name"], r["playback"]) for r in b["stateAnimations"]["AIR"]],
                             [("fall", "holdFinalFrame"), ("parry", "once"), ("unavailable", "loop")])
            self.assertIsNone(b["clipTiming"]["unavailable"])
            self.assertEqual(b["unresolvedAnimations"], [{"machine": "example", "state": "AIR", "name": "unavailable"}])

    def test_hilts_keep_each_mount_and_socket_with_its_own_visible_blades(self):
        from zero_hash import crc
        from zero_saber import saber_hilts
        from xform import IDENTITY, mat4
        turn = mat4([0, 1, 0, 0, 0, 1, 1, 0, 0, 2, 3, 4])
        rigs = {"bar": {"names": ["A", "B"], "model": [turn, IDENTITY]},
                "cup": {"names": ["C"], "model": [turn]}}
        hand = {"names": ["palm"], "parents": ["wrist"], "local": [turn]}
        joint = {"boneCrc": f"{crc('thumb'):08x}", "parentCrc": f"{crc('palm'):08x}",
                 "position": [0, 3, 1], "rotation": [0, 0, 1, 0]}
        animation = {"name": "hand", "joints": [joint]}
        images = {"mint": (2, 1, bytes([255, 0, 0, 0, 9, 52, 14, 252]))}
        weapon = {"class": "foil", "properties": [
            ("GeometryName", "BAR"), ("FirePointName", "A palm"), ("LightSaberTexture", "Mint"),
            ("LightSaberTrailColor", "9 18 32 71"), ("LightSaberLength", "0.23"),
            ("FirePointName", "B"), ("LightSaberTexture", "Mint"), ("LightSaberLength", "0.34"),
            ("OffhandGeometryName", "cup"), ("OffhandFirePointName", "C thumb"), ("LightSaberWidth", "0.031"),
            ("LightSaberTexture", "Mint"), ("AttachedFirePoint", "shin 0 1 0 3 4 5")]}
        a, b = saber_hilts(weapon, hand, animation, rigs, rigs, images)
        self.assertEqual((a["model"], a["attachment"]["local"]), ("bar", turn))
        self.assertEqual([e["transform"] for e in a["blades"]], [turn, IDENTITY])
        self.assertEqual([e["length"] for e in a["blades"]], [0.23, 0.34])
        self.assertEqual(a["blades"][0]["color"], (9, 52, 14, 252))
        self.assertEqual(a["blades"][0]["trailColor"], [9, 18, 32, 71])
        self.assertEqual(b["attachment"], {"name": "thumb", "skeleton": "hand", **joint})
        self.assertEqual([(e["socket"], e["texture"], e["width"]) for e in b["blades"]], [("c", "Mint", 0.031)])
        with self.assertRaisesRegex(ValueError, "foil: missing hilt socket cup/c"):
            saber_hilts(weapon, hand, animation, rigs, {**rigs, "cup": rigs["bar"]}, images)

    def test_prop_joint_streams_use_matching_indices_and_weights(self):
        from extract_swbf2 import _write_prop_attributes
        with tempfile.TemporaryDirectory() as tmp:
            records = {"mesh": [{}, {}]}
            parts = [{"joints": [1, 0]}, {"skinJoints": [1, 0, 2], "skinWeights": [0.5, 0.25, 0.25]}]
            from xform import IDENTITY
            rig = {"names": ["a", "b", "c"], "parents": ["", "a", "a"], "local": [IDENTITY] * 3}
            data = _write_prop_attributes({"mesh": parts}, records, {"mesh": rig}, Path(tmp))
            buf = (Path(tmp) / data["attributesFile"]).read_bytes()
            a, b = records["mesh"]
            self.assertEqual([r["influencesPerVertex"] for r in (a, b)], [1, 3])
            self.assertEqual(struct.unpack_from("<2I", buf, a["vertexStreams"]["joints"]), (1, 0))
            self.assertEqual(struct.unpack_from("<3I", buf, b["vertexStreams"]["joints"]), (1, 0, 2))
            self.assertEqual(struct.unpack_from("<3f", buf, b["vertexStreams"]["weights"]), (0.5, 0.25, 0.25))

    def test_untextured_marker_resolves_after_real_texture_lookup(self):
        from extract_swbf2 import _fix_material_textures
        def part(name):
            return {"texture": name, "material": {}}
        parts = [part("noIcon"), part("unavailable"), part(""), part("panel.tga")]
        self.assertEqual(_fix_material_textures(({"shell": parts},), {"Panel": object()}), {"unavailable"})
        self.assertEqual(parts[0], {"texture": "", "material": {"textureMode": "none", "sourceTexture": "noIcon"}})
        self.assertEqual(parts[1]["material"], {"textureMode": "missing", "missingTexture": "unavailable"})
        self.assertEqual(parts[2]["material"], {"textureMode": "none"})
        self.assertEqual(parts[3], {"texture": "Panel", "material": {"textureMode": "image"}})
        supplied = part("noIcon")
        _fix_material_textures(({"shell": [supplied]},), {"NOICON": object()})
        self.assertEqual(supplied, {"texture": "noIcon", "material": {"textureMode": "image"}})

    def test_linear_yaw_bounds_a_forward_semicircle(self):
        from extract_swbf2 import _motion_radius
        def key(t, v):
            return {"time": t, "type": 1, "value": v}
        clip = {"localTranslation": True, "positions": [key(0, [0, 0, 0]), key(1, [0, 0, math.pi])],
                "rotations": [key(0, [0, 0, 0]), key(1, [0, math.pi, 0])]}
        reach, planar = _motion_radius(clip)
        self.assertTrue(planar)
        self.assertAlmostEqual(reach, 2)

    def test_extract_map_base_layers_lights_paths_portals_bounds(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "SWBF2"
            game = root / "GameData" / "data" / "_lvl_pc"
            out = Path(tmp) / "out"
            write(game / "core.lvl", locl_archive(english={"entity.all.hero_test": "Test Hero"}))
            write(game / "common.lvl", ucfb())
            write(game / "ingame.lvl", ucfb())

            # Synthetic space map archive with void tern, static hull prop, ship system armedbuilding,
            # gameplay mapbounds region, ambient soundstream sphere (excluded from wireBounds),
            # lights, paths, portals, and a mode layer spa9_obj with a command post.
            hull_cls = _make_entc("spa_hull", "prop", [("GeometryName", "spa_hull")])
            sys_cls = _make_entc("spa_sys", "armedbuilding", [("GeometryName", "spa_HULL"), ("MaxHealth", "5000")])
            cp_cls = _make_entc("com_cp", "commandpost", [("GeometryName", "spa_hull")])
            base_wrld = chunk(
                b"wrld",
                chunk(b"NAME", b"spa9\x00")
                + chunk(b"TNAM", b"spa9\x00")
                + _make_inst("spa_hull", "hull_1", (1000.0, 20.0, -1500.0))
                + _make_inst("spa_sys", "engine_1", (-800.0, 50.0, 900.0))
                + _make_regn("box", "mapbounds", (0.0, 0.0, 0.0), (1200.0, 100.0, 1600.0))
                + _make_regn("sphere", "soundstream amb_space 1.0", (0.0, 0.0, 0.0), (9000.0, 9000.0, 9000.0)),
            )
            layer_wrld = chunk(
                b"wrld",
                chunk(b"NAME", b"spa9_obj_spawn\x00")
                + _make_inst("com_cp", "cp1", (10.0, 0.0, 20.0), [("Team", "1"), ("SpawnPath", "cp1_spawn")]),
            )
            lght_chunk = chunk(
                b"lght",
                chunk(b"NAME", struct.pack("<I", fnv1a("spa9")))
                + cfg_data("Light", "sun")
                + cfg_scop(cfg_data("Type", 1.0), cfg_data("Color", 1.0, 1.0, 0.9), cfg_data("Rotation", 1.0, 0.0, 0.0, 0.0))
                + cfg_data("GlobalLights")
                + cfg_scop(cfg_data("Light1", "sun"), cfg_data("Top", 64.0, 64.0, 64.0)),
            )
            path_chunk = chunk(
                b"path",
                cfg_data("Version", 10.0)
                + cfg_data("PathCount", 1.0)
                + cfg_data("Path", "cp1_spawn")
                + cfg_scop(
                    cfg_data("Nodes", 1.0),
                    cfg_scop(cfg_data("Node"), cfg_scop(cfg_data("Position", 10.0, 0.0, 20.0), cfg_data("Rotation", 1.0, 0.0, 0.0, 0.0))),
                ),
            )
            port_chunk = chunk(
                b"port",
                cfg_data("Sector", "hangar")
                + cfg_scop(cfg_data("Base", -5.0), cfg_data("Height", 20.0), cfg_data("Point", 0.0, 0.0), cfg_data("Object", "cp1")),
            )
            bone_xfrm = lambda x: struct.pack("<12f", 1, 0, 0, 0, 1, 0, 0, 0, 1, x, 0, 0)
            rig = chunk(b"skel", chunk(b"INFO", b"SPA_HULL\0" + struct.pack("<H", 2))
                        + chunk(b"NAME", b"root\0panel\0") + chunk(b"PRNT", b"\0root\0")
                        + chunk(b"XFRM", bone_xfrm(3) + bone_xfrm(5)))
            map_lvl = ucfb(
                _make_tex("hull_tex"),
                rig, _make_modl_swbf2("spa_hull", "hull_tex", attachment="panel"),
                chunk(b"tern", chunk(b"NAME", b"spa9\x00")),
                hull_cls,
                sys_cls,
                cp_cls,
                base_wrld,
                lght_chunk,
                path_chunk,
                port_chunk,
                lvl("spa9_obj", layer_wrld),
            )
            write(game / "spa" / "spa9.lvl", map_lvl)

            flight_manifest = {
                "calls": [
                    {"function": "SetMinFlyHeight", "args": [-450.0]},
                    {"function": "SetMaxFlyHeight", "args": [650.0]},
                ]
            }

            import extract_swbf2
            import extract as extract_mod
            with mock.patch.dict(os.environ, {"BFE_FAMILY": "swbf2"}):
                with mock.patch.object(extract_swbf2, "GAME", game), \
                     mock.patch.object(extract_swbf2, "PUBLIC", out), \
                     mock.patch.object(extract_mod, "GAME", game), \
                     mock.patch.object(extract_mod, "PUBLIC", out):
                    extract_swbf2.extract_map_swbf2("spa9", [], manifest=flight_manifest)

            mj = json.loads((out / "maps" / "spa9" / "map.json").read_text())
            self.assertEqual(mj["family"], "swbf2")
            self.assertEqual(mj["name"], "spa9")
            self.assertEqual(len(mj["lights"]), 1)
            self.assertIn("cp1_spawn", mj["spawns"])
            self.assertIn("portals", mj)
            self.assertEqual(len(mj["portals"]["sectors"]), 1)
            self.assertEqual(mj["wireBounds"], mj["bounds"])
            # Encloses mapbounds corners (±1200, ±1600) and manifest flight heights (-450..650),
            # while ignoring the 9000 m ambient soundstream sphere.
            self.assertEqual(mj["wireBounds"]["min"], [-1200.0, -450.0, -1600.0])
            self.assertEqual(mj["wireBounds"]["max"], [1200.0, 650.0, 1600.0])
            self.assertEqual(mj["layerIndex"][f"{fnv1a('spa9_obj'):08x}"], "spa9_obj")
            self.assertEqual(mj["layerErrors"], [])
            self.assertIn("spa9_obj", mj["layers"])
            self.assertEqual(len(mj["layers"]["spa9_obj"]["commandPosts"]), 1)
            self.assertIn("cp1_spawn", mj["layers"]["spa9_obj"]["spawns"])
            self.assertIn("spa_sys", mj["classes"])
            pack = json.loads((out / "maps/spa9/props.json").read_text())
            self.assertEqual(pack["instances"][0]["model"], "spa_hull")
            self.assertIn(pack["instances"][0]["model"], pack["models"])
            self.assertEqual(dict(mj["classes"]["spa_sys"]["properties"])["GeometryName"], "spa_HULL")
            bones = pack["skeletons"]["spa_hull"]
            self.assertEqual([(b["name"], b["parent"], b["local"][12]) for b in bones],
                             [("root", -1, 3), ("panel", 0, 5)])
            part, = pack["models"]["spa_hull"]
            self.assertEqual(part["bindBone"], 1)
            folder = out / "maps/spa9"
            self.assertEqual(struct.unpack_from("<3f", (folder / "props.bin").read_bytes(), part["vertexOffset"]), (8, 0, 0))
            self.assertEqual((folder / "props.bin").stat().st_size, 3 * 32)
            attrs = (folder / pack["attributesFile"]).read_bytes()
            for key, expected in (("tint", (1, 0, 0, 1)), ("lit", (0, 1, 0, 1)),
                                  ("tangent", (1, 0, 0)), ("binormal", (0, 0, 1))):
                self.assertEqual(struct.unpack_from(f"<{len(expected)}f", attrs, part["vertexStreams"][key]), expected)

    def test_extract_side_resolves_2005_inheritance_and_manifest(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "SWBF2"
            game = root / "GameData" / "data" / "_lvl_pc"
            out = Path(tmp) / "out"
            write(game / "core.lvl", locl_archive(english={"entity.crew.runner": "Relay Runner"}))

            # A shared loadout on a unit whose parent is in another package.
            parent_cls = _make_entc("relay_member", "soldier", [("EnergyRestore", "27.1"), ("MaxSpeed", "3.78")])
            child_cls = _make_entc(
                "crew_runner",
                "relay_member",
                [("GeometryName", "Relay_Mesh"), ("MaxSpeed", "6.23"), ("WeaponName1", "wick")],
            )
            wpn_child = _make_wpnc("wick", "torch_mount", [("LightSaberTrailColor", "12 49 101 19"), ("GeometryName", "RELAY_MESH")])

            wpn_parent = _make_wpnc("torch_mount", "melee", [
                ("LightSaberLength", "1.4"), ("OrdnanceName", "spark"), ("SecondaryOrdnanceName", "loose_pin")])
            spark = _make_entc("spark", "prop", [("ExplosionImpact", "glimmer")])
            ingame = ucfb(wpn_parent, spark)
            side = ucfb(lvl("crew_runner", _make_tex("brass"), _make_modl_swbf2("relay_mesh", "brass", sphere=(1.0, 1.0, 0.0, 1.375)), child_cls, wpn_child, _make_entc("orphan", "missing_parent", [])),
                        lvl("other_package", parent_cls, _make_entc("loose_pin", "prop", [])))
            write(game / "ingame.lvl", ingame)
            write(game / "side" / "crew.lvl", side)

            manifest = {
                "calls": [
                    {"function": "ReadDataFile", "args": ["SIDE\\crew.lvl", "crew_runner", "orphan"]},
                ]
            }
            import extract_swbf2
            import extract as extract_mod
            with mock.patch.dict(os.environ, {"BFE_FAMILY": "swbf2"}):
                with mock.patch.object(extract_swbf2, "GAME", game), \
                     mock.patch.object(extract_swbf2, "PUBLIC", out), \
                     mock.patch.object(extract_mod, "GAME", game), \
                     mock.patch.object(extract_mod, "PUBLIC", out):
                    extract_swbf2.extract_side_swbf2("crew", [], [ingame], manifest=manifest)

            uj = json.loads((out / "sides" / "crew" / "units.json").read_text())
            self.assertIsNone(uj["units"]["orphan"]["base"])
            self.assertEqual(uj["unresolvedReferences"], [
                {"weapon": "wick", "class": "spark", "property": "ExplosionImpact", "name": "glimmer"},
                {"weapon": "wick", "class": "wick", "property": "SecondaryOrdnanceName", "name": "loose_pin"}])
            self.assertEqual(uj["unresolvedParents"], [{"kind": "entc", "class": "orphan",
                                                        "parent": "missing_parent", "reason": "missing"}])
            catalog = json.loads((out / "sides/crew/classes.json").read_text())
            self.assertEqual(catalog["entc"]["relay_member"]["base"], "soldier")
            self.assertIn("crew_runner", uj["units"])
            runner = uj["units"]["crew_runner"]
            self.assertEqual(runner["base"], "soldier")
            self.assertEqual(runner["bounds"], [[0, 0, 0], [2, 2, 0]])
            self.assertEqual(runner["sphere"], [1, 1, 0, 1.375])
            self.assertEqual(runner["parents"], ["relay_member"])
            props = dict(runner["properties"])
            self.assertEqual(props["EnergyRestore"], "27.1")
            self.assertEqual(props["MaxSpeed"], "6.23")
            self.assertEqual(len(runner["weapons"]), 1)
            self.assertEqual(runner["weapons"][0]["base"], "melee")
            self.assertEqual(runner["weapons"][0]["LightSaberLength"], "1.4")
            model = runner["weapons"][0]["GeometryName"]
            self.assertEqual(runner["model"], model)
            self.assertEqual(props["GeometryName"], "Relay_Mesh")
            self.assertIn(model, json.loads((out / "sides/crew/weapons.json").read_text())["models"])


if __name__ == "__main__":
    unittest.main()
