"""Synthetic unit tests for 2005 (SWBF2) chunk readers in tools/zero_swbf2.py,
tools/zero_model.py (72-byte INFO, 24+B MTRL), tools/zero_texture.py (L8/A8L8),
and tools/ucfb.py (iter_all_archives / named_subarchives). Nothing retail."""

import zlib
import io
import struct
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "tools"))
sys.path.insert(0, str(HERE))
from test_addon_import import chunk, ucfb  # noqa: E402
import ucfb as ucfb_mod  # noqa: E402
from zero_hash import fnv1a  # noqa: E402
import zero_model  # noqa: E402
import zero_swbf2  # noqa: E402
import zero_texture  # noqa: E402


def cfg_data(name, *values):
    """Build one Zero config DATA chunk from floats and/or strings."""
    key = fnv1a(name)
    count = len(values)
    strings = bytearray()
    offsets = []
    words = []
    for val in values:
        if isinstance(val, str):
            offsets.append(4 * count + len(strings))
            words.append(struct.pack("<I", offsets[-1]))
            strings.extend(val.encode("latin1") + b"\x00")
        else:
            words.append(struct.pack("<f", float(val)))
    body = struct.pack("<IB", key, count) + b"".join(words)
    if strings:
        body += struct.pack("<I", len(strings)) + bytes(strings)
    return chunk(b"DATA", body)


def cfg_scop(*inner):
    return chunk(b"SCOP", b"".join(inner))


def lvl(name, *inner):
    body = b"".join(inner)
    return chunk(b"lvl_", struct.pack("<II", fnv1a(name), len(body)) + body)


class Swbf2ReaderTests(unittest.TestCase):
    def test_swbf2_mtrl_and_72byte_model_bounds(self):
        raw_flags = zero_model.SWBF2_NORMAL | zero_model.SWBF2_SPECULAR | zero_model.SWBF2_DOUBLESIDED | zero_model.SWBF2_ATTACHED_LIGHT
        mtrl_bytes = struct.pack("<6I", raw_flags, 0xFF112233, 0xFF445566, 32, 0, 0) + b"point_01\x00"
        self.assertNotIn("swbf2Flags", zero_model.parse_material(mtrl_bytes, 0, len(mtrl_bytes)))
        mat = zero_model.parse_material(mtrl_bytes, 0, len(mtrl_bytes), swbf2=True)
        self.assertTrue(mat["flags"] & zero_model.LIT)
        self.assertTrue(mat["flags"] & zero_model.SPECULAR)
        self.assertTrue(mat["doubleSided"])
        combined = struct.pack("<6I", zero_model.SWBF2_TRANSPARENT | zero_model.SWBF2_DOUBLESIDED,
                               0, 0, 0, 0, 0)
        self.assertFalse(zero_model.parse_material(combined, 0, len(combined), swbf2=True)["flags"]
                         & zero_model.TRANSPARENT)
        self.assertEqual(mat["attachedLight"], "point_01")
        self.assertEqual(mat["specular"][0], 32.0)

        # 72-byte INFO: 4 ints, vertex_box (6 floats), vis_box (6 floats), 2 ints
        info72 = struct.pack("<4i6f6f2i", 1, 2, 3, 4, -1, -2, -3, 1, 2, 3, -5, -6, -7, 5, 6, 7, 0, 12)
        modl = chunk(b"modl", chunk(b"INFO", info72))
        bounds = zero_model.model_bounds(modl, 8, len(modl), swbf2=True)
        self.assertEqual(bounds["box"], [[-5.0, -6.0, -7.0], [5.0, 6.0, 7.0]])

    def test_surface_intent_from_native_materials(self):
        z = zero_model
        for raw, double, blend, clip in [
            (0, False, "opaque", None),
            (z.SWBF2_TRANSPARENT, False, "alpha", None),
            (z.SWBF2_DOUBLESIDED, True, "alpha", None),
            (z.SWBF2_TRANSPARENT | z.SWBF2_DOUBLESIDED, True, "alpha", None),
            (z.SWBF2_HARD_EDGED | z.SWBF2_DOUBLESIDED, True, "opaque", 0.5),
            (z.SWBF2_HARD_EDGED, False, "opaque", 0.5),
            (z.SWBF2_ADDITIVE | z.SWBF2_TRANSPARENT, False, "additive", None),
        ]:
            with self.subTest(raw=raw):
                data = struct.pack("<6I", raw, 0, 0, 0, 0, 0)
                material = z.parse_material(data, 0, len(data), swbf2=True)
                self.assertEqual((material["doubleSided"], material["blendMode"], material["alphaCutout"]),
                                 (double, blend, clip))

    def test_scroll_speed_uses_the_unsigned_byte_from_each_word(self):
        from zero_model import parse_material, SWBF2_SCROLLING
        for words, expected in [((-128, -1), [128 / 255, 1]), ((203, 51), [203 / 255, 0.2])]:
            data = struct.pack("<4I2i", SWBF2_SCROLLING, 0, 0, 0, *words)
            self.assertEqual(parse_material(data, 0, len(data), swbf2=True)["scroll"], expected)
        for words in [(256, 0), (51, -129), (0, 1 << 30)]:
            data = struct.pack("<4I2i", SWBF2_SCROLLING, 0, 0, 0, *words)
            with self.assertRaisesRegex(ValueError, "Invalid 2005 MTRL scroll words"):
                parse_material(data, 0, len(data), swbf2=True)

    def test_sign_and_zero_extension_have_the_same_scroll_rate(self):
        from zero_model import parse_material, SWBF2_SCROLLING

        def speed(u, v):
            data = struct.pack("<4I2i", SWBF2_SCROLLING, 0, 0, 0, u, v)
            return parse_material(data, 0, len(data), swbf2=True)["scroll"]

        for high_byte in range(128, 256):
            self.assertEqual(speed(high_byte - 256, 17), speed(high_byte, 17))
            self.assertEqual(speed(17, high_byte - 256), [17 / 255, high_byte / 255])

    def test_l8_and_a8l8_textures(self):
        l8 = zero_texture.decode_pixels(zero_texture.L8, bytes([10, 200]), 2, 1)
        self.assertEqual(bytes(l8), bytes([10, 10, 10, 255, 200, 200, 200, 255]))
        a8l8 = zero_texture.decode_pixels(zero_texture.A8L8, bytes([50, 128, 255, 0]), 2, 1)
        self.assertEqual(bytes(a8l8), bytes([50, 50, 50, 128, 255, 255, 255, 0]))

    def test_parse_lights(self):
        body = (
            chunk(b"NAME", struct.pack("<I", fnv1a("tat2")))
            + cfg_data("Light", "sun")
            + cfg_scop(
                cfg_data("Type", 1.0),
                cfg_data("Color", 1.0, 0.9, 0.8),
                cfg_data("Position", 10.0, 20.0, 30.0),
                cfg_data("Rotation", 1.0, 0.0, 0.0, 0.0),
                cfg_data("CastShadow"),
                cfg_data("Region", "reg_a"),
            )
            + cfg_data("GlobalLights")
            + cfg_scop(
                cfg_data("Light1", "sun"),
                cfg_data("Top", 255.0, 0.0, 0.0),
                cfg_data("Bottom", 0.0, 255.0, 0.0),
            )
        )
        res = zero_swbf2.parse_lights(body, 0, len(body))
        self.assertEqual(len(res["lights"]), 1)
        sun = res["lights"][0]
        self.assertEqual(sun["name"], "sun")
        self.assertEqual(sun["type"], "directional")
        self.assertTrue(sun["castShadow"])
        self.assertEqual(sun["region"], "reg_a")
        self.assertEqual(sun["direction"], [0.0, 0.0, -1.0])
        self.assertEqual(res["global"]["light1"], "sun")
        self.assertEqual(res["global"]["top"], [1.0, 0.0, 0.0])

    def test_parse_paths_and_portals(self):
        pbody = (
            cfg_data("Version", 10.0)
            + cfg_data("PathCount", 1.0)
            + cfg_data("Path", "CP1_Spawn")
            + cfg_scop(
                cfg_data("SplineType", "Hermite"),
                cfg_data("Nodes", 1.0),
                cfg_scop(
                    cfg_data("Node"),
                    cfg_scop(
                        cfg_data("Position", 5.0, 2.0, -3.0),
                        cfg_data("Rotation", 1.0, 0.0, 0.0, 0.0),
                    ),
                ),
            )
        )
        by_name, detail = zero_swbf2.parse_paths(pbody, 0, len(pbody))
        self.assertIn("CP1_Spawn", by_name)
        self.assertEqual(by_name["CP1_Spawn"][0][:3], [5.0, 2.0, 3.0])
        self.assertAlmostEqual(by_name["CP1_Spawn"][0][3], 3.14159265)
        self.assertEqual(detail[0]["nodes"][0]["rotation"], [0., 0., 1., 0.])
        self.assertEqual(detail[0]["spline"], "Hermite")

        port_body = (
            cfg_data("Sector", "hangar")
            + cfg_scop(
                cfg_data("Base", -10.0),
                cfg_data("Height", 25.0),
                cfg_data("Point", 0.0, 0.0),
                cfg_data("Point", 10.0, 0.0),
                cfg_data("Object", "ship_cp"),
            )
            + cfg_data("Portal", "gate")
            + cfg_scop(
                cfg_data("Position", 5.0, 0.0, 0.0),
                cfg_data("Width", 20.0),
                cfg_data("Height", 12.0),
                cfg_data("Sector1", "hangar"),
                cfg_data("Sector2", "space"),
            )
        )
        port = zero_swbf2.parse_portals(port_body, 0, len(port_body))
        self.assertEqual(len(port["sectors"]), 1)
        self.assertEqual(port["sectors"][0]["objects"], ["ship_cp"])
        self.assertEqual(len(port["portals"]), 1)
        self.assertEqual(port["portals"][0]["sector2"], "space")

    def test_parse_cloth_combo_zaf_and_plan_v2(self):
        # CLTH
        cdata = (
            b"cape_tex\x00"
            + struct.pack("<I", 3)
            + struct.pack("<9f", 0, 0, 0, 1, 0, 0, 0, 1, 0)
            + struct.pack("<6f", 0, 0, 1, 0, 0, 1)
            + struct.pack("<I", 1)  # fixed points
            + struct.pack("<I", 1) + b"bone_spine\x00"
            + struct.pack("<I3I", 1, 0, 1, 2)  # 1 tri
            + struct.pack("<I2I", 1, 0, 1)  # stretch
            + struct.pack("<I", 0)  # bend
            + struct.pack("<I", 0)  # cross
        )
        ccoll = struct.pack("<I", 1) + b"bone_pelvis\x00" + struct.pack("<I15f", 1, 0.5, 0.5, 0.5, *([0.0] * 12))
        clth = (
            chunk(b"INFO", b"hero_body\x00")
            + chunk(b"NAME", b"cape\x00")
            + chunk(b"PRNT", b"bone_spine\x00")
            + chunk(b"XFRM", struct.pack("<12f", 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0))
            + chunk(b"DATA", cdata)
            + chunk(b"COLL", ccoll)
        )
        parsed_c = zero_swbf2.parse_cloth(clth, 0, len(clth))
        self.assertEqual(parsed_c["model"], "hero_body")
        self.assertEqual(parsed_c["texture"], "cape_tex")
        self.assertEqual(len(parsed_c["positions"]), 3)
        self.assertEqual(parsed_c["fixedWeights"], ["bone_spine"])
        self.assertEqual(parsed_c["triangles"], [[0, 1, 2]])
        self.assertEqual(len(parsed_c["collision"]), 1)

        # comb
        comb_body = (
            chunk(b"NAME", struct.pack("<I", fnv1a("hero_combo")))
            + cfg_data("OffhandAnimation", "stand_useforce", "FIRE")
            + cfg_data("State", "ATTACK1")
            + cfg_scop(cfg_data("Duration", 0.5), cfg_data("Damage", 100.0))
        )
        parsed_comb = zero_swbf2.parse_combo(comb_body, 0, len(comb_body))
        self.assertEqual(len(parsed_comb["offhandAnimations"]), 1)
        self.assertEqual(parsed_comb["states"][0]["name"], "ATTACK1")

        # zaf_
        tnoj_body = struct.pack("<I4f3f3I", 0x11223344, 0, 0, 0, 1, 0, 1, 0, 0x55667788, 0, 0)
        leks = b"LEKS" + struct.pack("<I", 8 + len(tnoj_body)) + b"TNOJ" + struct.pack("<I", len(tnoj_body)) + tnoj_body
        srev = b"SREV" + struct.pack("<II", 4, 8)
        ffaz = b"FFAZ" + struct.pack("<I", len(srev) + len(leks)) + srev + leks
        zaf = chunk(b"NAME", b"hero_skel\x00") + chunk(b"BIN_", ffaz)
        parsed_zaf = zero_swbf2.parse_zaf(zaf, 0, len(zaf))
        self.assertEqual(parsed_zaf["version"], 8)
        self.assertEqual(len(parsed_zaf["joints"]), 1)
        self.assertEqual(parsed_zaf["joints"][0]["boneCrc"], "11223344")

        # plan v2
        hub0 = b"hub0".ljust(16, b"\x00") + struct.pack("<4f", 1, 2, 3, 4) + bytes([0] + [255] * 7) + bytes([0, 0, 0, 0, 0])
        hub1 = b"hub1".ljust(16, b"\x00") + struct.pack("<4f", 5, 6, 7, 4) + bytes([0] + [255] * 7) + bytes([0, 0, 0, 0, 0])
        arc0 = b"arc0".ljust(16, b"\x00") + struct.pack("<BBII", 0, 1, 1, 5)
        plan = chunk(b"INFO", struct.pack("<HHH", 2, 1, 5)) + chunk(b"NODE", hub0 + hub1) + chunk(b"ARCS", arc0)
        parsed_p = zero_swbf2.parse_plan_v2(plan, 0, len(plan))
        self.assertEqual(parsed_p["hubCount"], 2)
        self.assertEqual(parsed_p["arcs"][0]["oneWay"], True)
        self.assertEqual(parsed_p["arcs"][0]["jetJump"], True)

    def test_combo_json_preserves_argument_types_and_scoped_reaction_metadata(self):
        import json
        tree = (
            cfg_data("OffhandAnimation", "fold") + cfg_scop(cfg_data("AimType", "Pivot"))
            + cfg_data("State", "LATCH") + cfg_scop(
                cfg_data("Attack") + cfg_scop(cfg_data("DamageTime", "0.875", 0.875))
                + cfg_data("Transition", "REST") + cfg_scop(cfg_data("TimeStart", "-3.25", "Frames"),
                                                          cfg_data("TimeEnd", -3.25, "Frames"))
                + cfg_data("Deflect") + cfg_scop(cfg_data("DeflectAnimation", "tilt") + cfg_scop(
                    cfg_data("BlendOutTime", 0.6875), cfg_data("LowResPose", 1.375, "Seconds"),
                    cfg_data("BlendInTime", 0.3125)))
            )
        )
        machine = json.loads(json.dumps(zero_swbf2.parse_combo(tree, 0, len(tree))))
        self.assertEqual(machine["offhandAnimations"][0]["children"], [{"key": "AimType", "values": ["Pivot"]}])
        attack, transition, deflect = machine["states"][0]["children"]
        self.assertEqual(attack["children"][0]["values"], ["0.875", 0.875])
        self.assertEqual([n["values"] for n in transition["children"]], [["-3.25", "Frames"], [-3.25, "Frames"]])
        self.assertEqual(deflect["children"][0]["children"], [
            {"key": "BlendOutTime", "values": [0.6875]}, {"key": "LowResPose", "values": [1.375, "Seconds"]},
            {"key": "BlendInTime", "values": [0.3125]}])

    def test_bank_keeps_curve_values_and_bone_ids(self):
        def section(tag, data):
            return tag + struct.pack("<I", len(data)) + data
        curves = struct.pack("<hbhb", 0, 1, 2047, 0)
        header = b"SMNA" + struct.pack("<4I2H", 0, 8, 1, len(curves), 1, 0)
        record = struct.pack("<I7I4f", 123, 0, 0, 0, 3, 0, 0, 0, 1, 2, 3, .5)
        blob = (header + section(b"MINA", struct.pack("<IIIHH", 1, 0, 42, 2, 1))
                + section(b"TNJA", record) + section(b"TADA", curves))
        stream = io.BytesIO()
        clip = zero_swbf2._write_bank(blob, stream)["clips"][0]
        self.assertEqual((clip["tracks"], clip["offset"], clip["crc"]), (["0000007b"], 0, "0000002a"))
        samples = struct.unpack("<14f", stream.getvalue())
        self.assertEqual(samples[4:7], (1., 2., 3.))
        self.assertEqual(samples[11:], (1.5, 2.5, 3.5))
        with self.assertRaises(ValueError):
            zero_swbf2._write_bank(b"", stream)

    def test_planning_groups_reference_arcs_and_preserve_compressed_blocks(self):
        payload = zlib.compress(b"routes")
        body = struct.pack("<BBBI2I", 1, 1, 1, fnv1a("bridge"), 6, len(payload)) + payload
        index, data = zero_swbf2.parse_plnp(body, 0, len(body))
        self.assertEqual(index["connectionHashes"], [f"{fnv1a('bridge'):08x}"])
        self.assertEqual(index["blocks"], [{"offset": 0, "size": len(payload), "unpackedSize": 6}])
        self.assertEqual(zlib.decompress(data), b"routes")
        with self.assertRaises(ValueError):
            zero_swbf2.parse_plnp(body, 0, len(body) - 1)

    def test_animation_group_keeps_every_binding_and_key(self):
        head = chunk(b"INFO", b"motion\0" + struct.pack("<fBB", 4., 1, 1))
        key = struct.pack("<4fB6f", 4., 1., 2., 3., 1, *([0.] * 6))
        clip = chunk(b"anim", head + chunk(b"POSK", key))
        group = chunk(b"anmg", chunk(b"INFO", b"group\0\1\0") +
                      chunk(b"ANIM", b"motion\0left\0") + chunk(b"ANIM", b"motion\0right\0"))
        hierarchy = chunk(b"anmh", chunk(b"INFO", b"\2left\0child\0"))
        blob = clip + group + hierarchy
        motion = zero_swbf2.world_animations(blob, [("world", 0, len(blob))])
        self.assertEqual(motion["clips"]["motion"]["positions"][0]["value"], [1., 2., 3.])
        self.assertEqual([b["object"] for b in motion["groups"]["group"]["bindings"]], ["left", "right"])
        self.assertEqual(motion["hierarchies"], [{"parent": "left", "children": ["child"]}])

    def test_world_sections_splits_base_and_layers(self):
        arc = ucfb(
            chunk(b"wrld", chunk(b"NAME", b"tat2\x00")),
            lvl("tat2_tdm", chunk(b"wrld", chunk(b"NAME", b"tat2_tdm\x00"))),
        )
        ws = zero_swbf2.world_sections(arc, "tat2")
        self.assertEqual([w[0] for w in ws["base"]["worlds"]], ["tat2"])
        key = f"{fnv1a('tat2_tdm'):08x}"
        self.assertEqual(ws["layers"][key]["name"], "tat2_tdm")
        self.assertEqual([w[0] for w in ws["layers"][key]["worlds"]], ["tat2_tdm"])
        self.assertEqual(len(list(ucfb_mod.iter_all_archives(arc))), 2)

    def test_hash_matches_layer_parts_from_installed_scripts_and_worlds(self):
        import tempfile
        from zero_mission2 import layer_name_parts
        def constant(word):
            return b"\x04" + struct.pack("<I", len(word) + 1) + word + b"\0"
        body = b"\x1bLua\x50\x01\x04\x04" + b"".join(map(constant, (b"port8_race", b"pair", b"crew_pair_runner")))
        body += b"\x04\x17\0\0\0phantom\0"
        with tempfile.TemporaryDirectory() as tmp:
            mission = Path(tmp) / "scripts.lvl"
            mission.write_bytes(ucfb(lvl("menu", chunk(b"scr_", chunk(b"NAME", b"event\0") + chunk(b"BODY", body)))))
            parts = layer_name_parts(mission)
        self.assertEqual(parts, ({"race"}, {"pair"}))
        world = chunk(b"wrld", chunk(b"NAME", b"port9_jump\0"))
        arc = ucfb(lvl("port9_race", world), lvl("port9_pairjump", world), lvl("port9_other", world))
        sections = zero_swbf2.world_sections(arc, "port9", name_parts=parts, keep_unnamed=True)["layers"]
        for name in ("port9_race", "port9_pairjump"):
            self.assertEqual(sections[f"{fnv1a(name):08x}"]["name"], name)
        self.assertIsNone(sections[f"{fnv1a('port9_other'):08x}"]["name"])

    def test_archive_hash_resolves_a_recorder_only_package_and_diagnoses_absence(self):
        arc = ucfb(lvl("evening", chunk(b"wrld", chunk(b"NAME", b"arena_rooms\0"))))
        key = f"{fnv1a('evening'):08x}"
        with self.assertRaisesRegex(ValueError, key + ".*arena_rooms"):
            zero_swbf2.world_sections(arc, "arena")
        unnamed = zero_swbf2.world_sections(arc, "arena", keep_unnamed=True)["layers"][key]
        self.assertIsNone(unnamed["name"])
        self.assertIn("arena_rooms", unnamed["nameError"]["candidates"])
        named = zero_swbf2.world_sections(arc, "arena", ["Evening"])["layers"]
        self.assertEqual(zero_swbf2.requested_layers(named, ["evening"], "arena"), [named[key]])
        missing = f"{fnv1a('morning'):08x}"
        with self.assertRaisesRegex(ValueError, missing + ".*evening"):
            zero_swbf2.requested_layers(named, ["morning"], "arena")


if __name__ == "__main__":
    unittest.main()
