"""Bind geometry for independently movable 2005 vehicle parts."""

import json
from pathlib import Path
import struct
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))

from xform import IDENTITY, mul
from extract_swbf2 import _write_classes
from zero_hash import fnv1a
from zero_swbf2 import resolve_class
from zero_vehicle2 import pack_current, vehicle_pack, write_pack


def chunk(tag, data):
    return tag + struct.pack("<I", len(data)) + data + bytes(-len(data) % 4)


def shift(x, y, z):
    matrix = list(IDENTITY)
    matrix[12:15] = [x, y, z]
    return matrix


def sample_pack():
    root = shift(4, 8, -3)
    pivot = [0., 1., 0., 0., -1., 0., 0., 0., 0., 0., 1., 0., 2., 3., 4., 1.]
    socket = shift(2, 0, 1)
    rig = {"names": ["Cradle", "Turn", "Needle"], "parents": ["", "Cradle", "Turn"],
           "local": [root, pivot, socket], "model": [root, mul(root, pivot), mul(mul(root, pivot), socket)]}
    mesh = chunk(b"coll", chunk(b"NAME", b"loom\0") + chunk(b"NODE", b"Cradle\0") +
                 chunk(b"INFO", struct.pack("<I", 3)) +
                 chunk(b"POSI", struct.pack("<9f", -2, 0, 1, 3, 0, 2, 0, 6, -1)) +
                 chunk(b"TREE", chunk(b"LEAF", bytes([3])+bytes(6)+struct.pack("<3H", 2, 0, 1))))
    props = [("GeometryName", "Loom"), ("4cbe283c", "BASKET"), ("FireNodeName", "nEEdLe"),
             ("WeaponName", "spool"), ("EyePointOffset", "2 0 0"), ("TurretNodeName", "Turn"),
             ("NextAimer", "-"), ("AimerNodeName", "turn"), ("PilotPosition", "Cradle"),
             ("VehicleCollisionOnly", "drum"), ("DamageAttachPoint", "absent")]
    wp = {"name": "spool", "base": "launcher", "props": [("FirePointExtraOffset", "0 0 7")]}
    catalog = {"entc": {"carrier": {"name": "Carrier", "base": "flyer", "props": props}}, "wpnc": {"spool": wp}}
    cls = resolve_class("entc", "carrier", catalog)
    shape = {"name": "Drum", "parent": "tURn", "kind": 4, "size": [1, 2, 3], "matrix": shift(-2, 4, 6), "mask": 34}
    return vehicle_pack({"carrier": cls}, catalog, {"loom": rig}, {"loom": [shape]},
                        {"loom": True}, [(mesh, [(0, len(mesh))])])


class VehiclePackTests(unittest.TestCase):
    def test_turret_classes_and_buildings_with_named_controls_get_sections(self):
        def obj(name, base, props):
            return {"name": name, "base": base, "props": props}

        controls = [("BuildingSection", "BODY"), ("BuildingSection", "Loft"),
                    ("TurretSection", "Loft"), ("WeaponName", "spool"), ("FirePointName", "lip")]
        classes = {"entc": {"stand": obj("stand", "defensegridturret", controls),
                            "shed": obj("shed", "animatedbuilding", controls),
                            "wall": obj("wall", "building", [("BuildingSection", "BODY")])}}
        resolved = {n: resolve_class("entc", n, classes) for n in classes["entc"]}
        pack = vehicle_pack(resolved, classes, {}, {}, {}, [])
        self.assertEqual(set(pack["classes"]), {"stand", "shed"})
        for record in pack["classes"].values():
            body, station, turret = record["vehicleSections"]
            self.assertEqual((body["kind"], station["name"], turret["name"]), ("chassis", "Loft", "Loft"))
            seat, = record["seats"]
            weapon, = seat["weapons"]
            self.assertEqual((seat["name"], weapon["class"], weapon["firePoints"]), ("Loft", "spool", [0]))

    def test_debris_selection_preserves_the_active_flight_or_seat_controls(self):
        props = [("FlyerSection", "BODY"), ("WeaponName", "spool"), ("ChunkSection", "Cap"),
                 ("ChunkGeometryName", "cap"), ("ChunkSmokeNodeName", "wick"), ("TurnRate", ".47"),
                 ("FlyerSection", "Box"), ("WeaponName", "shuttle"), ("CHUNKSECTION", "Pedal"),
                 ("ChunkGeometryName", "pedal"), ("PitchRate", "1.9"), ("FireNodeName", "needle")]
        classes = {"entc": {"loom": {"name": "Loom", "base": "flyer", "props": props}}}
        cls = resolve_class("entc", "loom", classes)
        hull = vehicle_pack({"loom": cls}, classes, {}, {}, {}, [])["classes"]["loom"]
        self.assertEqual([(s["kind"], s["name"]) for s in hull["vehicleSections"]],
                         [("chassis", "BODY"), ("seat", "Box")])
        body, seat = hull["vehicleSections"]
        self.assertEqual(body["properties"], props[1:6])
        self.assertEqual(seat["properties"], props[7:])
        smoke, muzzle = hull["points"]
        self.assertEqual((smoke["name"], smoke["model"]), ("wick", "cap"))
        self.assertEqual((muzzle["name"], muzzle["model"]), ("needle", "loom"))
        self.assertNotIn(0, hull["seats"][0]["points"])
        self.assertEqual(hull["seats"][1]["weapons"][0]["firePoints"], [1])

    def test_inherited_class_sections_keep_order_and_reset_to_the_chassis(self):
        header = f"{fnv1a('FlyerSection'):08x}"
        original = [(header, "BODY"), ("TurnRate", ".24"), ("PitchRate", ".57"),
                    (header, "Perch"), ("TurnRate", "2.1"), ("PitchLimits", "-16 28"),
                    ("WeaponSection", "2"), ("WeaponName", "spindle"), ("PilotPosition", "nest"),
                    ("af03e2b9", "first"), ("af03e2b9", "second"), ("CHUNKSECTION", "Scrap")]
        changes = [("TurnRate", ".38"), ("WALKERSECTION", "Perch"), ("PitchRate", "1.7"),
                   ("WALKERSECTION", "Loft"), ("WALKERSECTION", "Perch"), ("YawLimits", "-11 32")]
        classes = {"entc": {"loom": {"name": "Loom", "base": "flyer", "props": original},
                            "loom_plus": {"name": "Loom_Plus", "base": "loom", "props": changes},
                            "weaver": {"name": "Weaver", "base": "soldier", "props": [("TurnRate", ".1")]}}}
        with tempfile.TemporaryDirectory() as folder:
            _write_classes(Path(folder), classes)
            entities = json.loads((Path(folder) / "classes.json").read_bytes())["entc"]
        self.assertNotIn("vehicleSections", entities["weaver"])
        self.assertEqual(entities["loom_plus"]["properties"], [list(pair) for pair in changes])
        sections = entities["loom_plus"]["vehicleSections"]
        self.assertEqual([(s["declaredBy"], s["kind"], s["name"], s["marker"]) for s in sections], [
            ("Loom", "chassis", "BODY", "FlyerSection"), ("Loom", "seat", "Perch", "FlyerSection"),
            ("Loom_Plus", "chassis", "BODY", None),
            ("Loom_Plus", "seat", "Perch", "WalkerSection"), ("Loom_Plus", "seat", "Loft", "WalkerSection"),
            ("Loom_Plus", "seat", "Perch", "WalkerSection")])
        self.assertEqual([pair for s in sections if s["kind"] == "chassis" for pair in s["properties"]],
                         [["TurnRate", ".24"], ["PitchRate", ".57"], ["TurnRate", ".38"]])
        self.assertEqual(sections[1]["properties"], [list(pair) for pair in original[4:]])
        self.assertEqual(sections[4]["properties"], [])
        self.assertEqual(sections[-1]["properties"], [["YawLimits", "-11 32"]])

    def test_building_seats_share_mounts_only_with_their_own_turret(self):
        props = [("BuildingSection", "BODY"), ("WeaponName", "shuttle"), ("FirePointName", "keel"),
                 ("BuildingSection", "Cup"), ("WeaponName", "spool"), ("TurnRate", "1.8"),
                 ("TurretSection", "CUP"), ("FirePointName", "rim"),
                 ("BuildingSection", "Tray"), ("WeaponName", "ribbon"), ("PilotPosition", "ledge"),
                 ("TurretSection", "Tray"), ("FirePointName", "lip")]
        classes = {"entc": {"stand": {"name": "Stand", "base": "armedbuilding", "props": props}}}
        cls = resolve_class("entc", "stand", classes)
        pack = vehicle_pack({"stand": cls}, classes, {}, {}, {}, [])
        hull = pack["classes"]["stand"]
        seats = {s["name"]: s for s in hull["seats"]}
        self.assertEqual(list(seats), ["BODY", "Cup", "Tray"])
        self.assertEqual(hull["properties"], props)
        self.assertEqual([s["name"] for s in hull["vehicleSections"]], ["BODY", "Cup", "CUP", "Tray", "Tray"])
        for seat, name, muzzle in [("BODY", "shuttle", "keel"), ("Cup", "spool", "rim"), ("Tray", "ribbon", "lip")]:
            weapon, = seats[seat]["weapons"]
            point, = weapon["firePoints"]
            self.assertEqual((weapon["section"], weapon["class"], hull["points"][point]["name"]), ("1", name, muzzle))
        self.assertIn("ledge", [hull["points"][i]["name"] for i in seats["Tray"]["points"]])

    def test_model_and_collision_keep_source_frames_and_masks(self):
        output = sample_pack()
        model = output["models"]["loom"]
        joint = model["skeleton"][1]
        self.assertEqual((joint["name"], joint["parent"]), ("Turn", 0))
        shape, = model["primitives"]
        self.assertEqual(shape["matrix"], mul(joint["matrix"], shape["local"]))
        self.assertEqual((shape["joint"], shape["size"], len(shape["indices"])), (1, [1, 2, 3], 36))
        self.assertEqual((shape["mask"], shape["collidesWith"]), (34, ["vehicle", "flyer"]))
        mesh, = model["meshes"]
        self.assertEqual((mesh["node"], mesh["joint"], mesh["mask"], len(mesh["collidesWith"])), ("Cradle", 0, None, 6))
        self.assertEqual(mesh["indices"], [2, 0, 1])
        self.assertEqual(mesh["positions"][1], (3, 0, 2))
        selected, = output["classes"]["carrier"]["collisionSelections"]
        self.assertEqual((selected["name"], selected["primitives"], selected["meshes"]), ("drum", [0], []))

    def test_muzzles_seats_and_offsets_use_their_own_bind_transforms(self):
        output = sample_pack()
        hull = output["classes"]["carrier"]
        seat = next(s for s in hull["seats"] if s["name"] == "BASKET")
        weapon, = seat["weapons"]
        first, second = [hull["points"][i] for i in weapon["firePoints"]]
        self.assertEqual((weapon["class"], first["joint"], first["parent"], second["joint"]), ("spool", 2, 1, 1))
        eye = next(p for p in hull["points"] if p["property"] == "EyePointOffset")
        self.assertEqual(eye["matrix"], mul(second["matrix"], shift(2, 0, 0)))
        for offset, origin in zip(weapon["offsetPoints"], (first, second)):
            self.assertEqual(hull["points"][offset["point"]]["matrix"], mul(origin["matrix"], shift(0, 0, 7)))
        self.assertEqual(hull["unresolvedPoints"], [{"property": "DamageAttachPoint", "name": "absent", "model": "loom"}])

    def test_resume_detects_missing_and_modified_vehicle_geometry(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            record = write_pack(root, sample_pack())
            (root / "map.json").write_text(json.dumps(record))
            self.assertTrue(pack_current(root))
            path = root / record["vehicles"]
            path.write_bytes(path.read_bytes().replace(b"Drum", b"Drab"))
            self.assertFalse(pack_current(root))
            path.unlink()
            self.assertFalse(pack_current(root))
