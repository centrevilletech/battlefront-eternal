"""Non-retail fixtures for polygon holes, source winding, and the model a
class's collision is filed under."""
import array
import importlib.util
import json
import sys
import tempfile
import unittest
from unittest.mock import patch
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from zero_world import triangulate_polygon
from zero_odf import property_name
from extract import (collision_model, collision_sources, map_archive, new_polys, place_collision, soldier_collision_exclusions,
                     write_collision, write_props)
from zero_odf import entity_classes
from zero_world import parse_instances
from ucfb import chunks, iter_archives
from xform import IDENTITY

_spec = importlib.util.spec_from_file_location("audit_collision", Path(__file__).resolve().parents[1] / "tools" / "audit-collision.py")
audit = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(audit)


class CollisionTriangulation(unittest.TestCase):
    def test_all_retail_maps_keep_placed_collision_with_class_spelling(self):
        maps = ("bes1", "bes2", "end1", "geo1", "hot1", "kam1", "kas1", "kas2",
                "nab1", "nab2", "rhn1", "rhn2", "tat1", "tat2", "yav1", "yav2")
        checked = 0
        for name in maps:
            if not map_archive(name).exists():
                continue
            data = map_archive(name).read_bytes()
            classes = entity_classes(data)
            canonical = {key.lower(): key for key in classes}
            meshes, shapes = collision_sources([data])
            excluded = soldier_collision_exclusions([data])
            placed = {canonical.get(kind.lower(), kind)
                      for start, end in iter_archives(data)
                      for tag, body, stop in chunks(data, start, end) if tag == b"wrld"
                      for kind, _ in parse_instances(data, body, stop)}
            count = 0
            for kind in placed:
                if kind.lower() not in meshes and kind.lower() not in shapes:
                    continue
                with self.subTest(map=name, model=kind):
                    expected, actual = new_polys(), new_polys()
                    for spelling, out in [(kind.lower(), expected), (kind, actual)]:
                        place_collision(spelling, IDENTITY, meshes, shapes, None, None, out,
                                        excluded.get(kind.lower(), ()))
                    self.assertEqual(actual, expected, "class spelling must not drop collision")
                count += 1
            self.assertGreater(count, 0, name)
            print(f"{name}: checked {count} placed collision model types")
            checked += 1
        if not checked:
            self.skipTest("local retail archives are not installed")

    def test_collision_resource_names_ignore_case_and_keep_archive_priority(self):
        stairs = [([(0, 0, 0), (0, 1, 2), (2, 1, 2)], [(0, 1, 2)])]
        pillar = [{"name": "P_SOLDIER", "kind": 4, "size": (0.3, 1, 0.3), "matrix": IDENTITY}]
        with patch("extract.coll_meshes", side_effect=[{"temple_stairs": stairs}, {"TEMPLE_STAIRS": []}]), \
             patch("extract.prim_shapes", side_effect=[{"Temple_Stairs": pillar}, {"TEMPLE_STAIRS": []}]):
            meshes, shapes = collision_sources([b"map", b"shared"])
        out = new_polys()
        place_collision("Temple_Stairs", IDENTITY, meshes, shapes, None, {"p_soldier"}, out)
        self.assertEqual(out["count"], 7, "mixed-case stairs retain mesh and soldier primitive")
        self.assertEqual(list(out["pos"][:9]), [0, 0, 0, 0, 1, 2, 2, 1, 2])

    def test_vehicle_only_metadata_and_preserved_mesh(self):
        # This exact retail ODF hash was previously unknown, so five large
        # vehicle boxes sealed the canal_home pedestrian arcades.
        self.assertEqual(property_name(0xC3F25CBB), "VehicleCollisionOnly")
        mesh = {"arcade": [([(-3, 0, 0), (-3, 4, 0), (-3, 0, 2)], [(0, 1, 2)])]}
        shapes = {"arcade": [{"name": "P_VEHICLE_01", "kind": 4, "size": (4, 4, 4), "matrix": IDENTITY},
                             {"name": "p_pillar", "kind": 4, "size": (0.3, 4, 0.3), "matrix": IDENTITY}]}
        out = new_polys()
        place_collision("arcade", IDENTITY, mesh, shapes, None, None, out, {"p_vehicle_01"})
        self.assertEqual(out["count"], 7)  # mesh face and ordinary pillar's six faces
        self.assertEqual(list(out["idx"][:3]), [0, 1, 2])

    def test_concave_arch_keeps_opening(self):
        # A U-shaped wall: its interior opening must never become collision.
        points = [(0, 0, 0), (4, 0, 0), (4, 4, 0), (3, 4, 0),
                  (3, 1, 0), (1, 1, 0), (1, 4, 0), (0, 4, 0)]
        for poly in [list(range(8)), list(reversed(range(8)))]:
            triangles = triangulate_polygon(points, poly)
            area = 0
            for a, b, c in triangles:
                a, b, c = points[a], points[b], points[c]
                signed = ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])) / 2
                self.assertGreater(signed * (1 if poly[0] == 0 else -1), 0)
                area += abs(signed)
            self.assertAlmostEqual(area, 10)
            self.assertEqual(len(triangles), 6)

    def test_vertical_and_horizontal_projection(self):
        for points in [[(0, 0, 0), (3, 0, 0), (3, 0, 1), (1, 0, 1), (1, 0, 3), (0, 0, 3)],
                       [(0, 0, 0), (0, 3, 0), (0, 3, 1), (0, 1, 1), (0, 1, 3), (0, 0, 3)]]:
            self.assertEqual(len(triangulate_polygon(points, list(range(6)))), 4)


def placed_at(x, y, z):
    """A world instance's raw XFRM: identity rotation, then the position."""
    return [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, float(x), float(y), float(z)]


# The shapes of the retail cases, in miniature: Yavin's tree class draws
# yav_prop_leaftree1 (trunk cylinders), Kashyyyk's platform class is named
# with its .msh and draws kas2_bldg_platform_1 (a coll mesh), the tower
# turret is the watchtower model (mesh for soldiers, a cylinder for
# vehicles), and the gun turret's class and model share a name.
CLASSES = {
    "yav_prop_leaftree": {"base": "prop", "props": [("GeometryName", "yav_prop_leaftree1"), ("AttachOdf", "yav_prop_leafpatch")]},
    "kas2_bldg_platform_1.msh": {"base": "prop", "props": [("GeometryName", "kas2_bldg_platform_1"), ("a1c9cbc5", "wood_foley")]},
    "yav_bldg_tower_turret": {"base": "armedbuilding", "props": [("GeometryName", "yav_bldg_watchtower"), ("VehicleCollision", "p_vehiclecylinder"),
                                                                 ("SoldierCollision", "CollisionMesh")]},
    "com_weap_gunturret": {"base": "armedbuilding", "props": [("MaxHealth", "500")]},
}
PLATFORM = [([(-2.0, 0.0, -2.0), (2.0, 0.0, -2.0), (2.0, 0.0, 2.0), (-2.0, 0.0, 2.0)], [(0, 1, 2, 3)])]
MESHES = {"kas2_bldg_platform_1": PLATFORM, "yav_bldg_watchtower": PLATFORM}
SHAPES = {
    "yav_prop_leaftree1": [{"name": "p_tree1", "parent": "yav_prop_leaftree", "kind": 2, "size": (0.9, 15.7, 0.0), "matrix": IDENTITY}],
    "yav_bldg_watchtower": [{"name": "p_vehiclecylinder", "parent": "yav_bldg_watchtower", "kind": 2, "size": (1.5, 15.4, 0.0), "matrix": IDENTITY}],
    "com_weap_gunturret": [{"name": "p_base", "parent": "com_weap_gunturret", "kind": 4, "size": (1.0, 1.0, 1.0), "matrix": IDENTITY}],
}
LIVE = {"yav_bldg_tower_turret", "com_weap_gunturret"}
SOLDIER_SHAPES = {"yav_bldg_tower_turret": {"CollisionMesh"}}


def read_floats(path):
    out = array.array("f")
    out.frombytes(path.read_bytes())
    return out


class CollisionGeometryName(unittest.TestCase):
    """Collision is filed under the model a class draws, not under the class."""

    def test_collision_model_follows_geometry_name(self):
        self.assertEqual(collision_model("yav_prop_leaftree", CLASSES, MESHES, SHAPES), "yav_prop_leaftree1")
        self.assertEqual(collision_model("kas2_bldg_platform_1.msh", CLASSES, MESHES, SHAPES), "kas2_bldg_platform_1")
        self.assertEqual(collision_model("yav_bldg_tower_turret", CLASSES, MESHES, SHAPES), "yav_bldg_watchtower")
        self.assertEqual(collision_model("com_weap_gunturret", CLASSES, MESHES, SHAPES), "com_weap_gunturret", "no GeometryName: the class's own name")
        self.assertEqual(collision_model("yav1_prop_shrub", CLASSES, MESHES, SHAPES), "yav1_prop_shrub", "an unknown class (a painted File mesh)")
        self.assertEqual(collision_model("yav_prop_leaftree", None, MESHES, SHAPES), "yav_prop_leaftree", "without classes, the class name")

    def test_write_collision_exports_the_models_collision_for_renamed_classes(self):
        instances = [("yav_prop_leaftree", placed_at(10, -70, 0)), ("kas2_bldg_platform_1.msh", placed_at(0, 18, 0))]
        with tempfile.TemporaryDirectory() as tmp, patch("extract.collision_sources", return_value=(MESHES, SHAPES)):
            out = Path(tmp)
            write_collision([b""], instances, None, out, {}, {}, LIVE, classes=CLASSES)
            positions = read_floats(out / "collision.bin")
            polygons = array.array("I")
            polygons.frombytes((out / "collision_poly.bin").read_bytes())
            self.assertEqual(max(polygons) + 1, 19, "the trunk's 16 sides and two caps, and the platform's face")
            vertices = [tuple(positions[i:i + 3]) for i in range(0, len(positions), 3)]
            self.assertTrue(any(abs(x - 10) <= 0.9 + 1e-6 and abs(z) <= 0.9 + 1e-6 and abs(y - (-70 + 7.85)) < 1e-4 for x, y, z in vertices),
                            "the trunk stands at the tree's position")
            self.assertTrue(any(abs(y - 18) < 1e-6 and abs(x) == 2 and abs(z) == 2 for x, y, z in vertices), "the platform's face at its height")
            # Looked up by the class name, as before, neither has any collision.
            write_collision([b""], instances, None, out, {}, {}, LIVE)
            self.assertEqual((out / "collision.bin").read_bytes(), b"")

    def test_write_props_files_a_live_objects_collision_under_its_model(self):
        records = [{"class": "yav_bldg_tower_turret", "name": "tower", "xfrm": placed_at(5, 0, 5), "props": []},
                   {"class": "com_weap_gunturret", "name": "gun", "xfrm": placed_at(-5, 0, 5), "props": []}]
        with tempfile.TemporaryDirectory() as tmp, patch("extract.collision_sources", return_value=(MESHES, SHAPES)):
            out = Path(tmp)
            write_props(records, CLASSES, LIVE, {}, {}, [b""], out, {}, SOLDIER_SHAPES)
            tower, gun = json.loads((out / "props.json").read_text())["instances"]
            self.assertEqual(tower["model"], "yav_bldg_watchtower")
            self.assertEqual(tower["collision"], [0, 2], "the watchtower mesh's two triangles; the vehicle cylinder is vehicles' alone")
            self.assertEqual(gun["collision"], [2, 12], "a box's six faces, under the class's own name")
            positions = read_floats(out / "props_collision.bin")
            self.assertEqual(list(positions[:3]), [3.0, 0.0, 3.0], "the mesh stands at the turret's position")

    def test_audit_reports_placed_objects_whose_collision_the_extract_lacks(self):
        instances = [("yav_prop_leaftree", placed_at(10, -70, 0)), ("yav_prop_leaftree", placed_at(30, -70, 0)),
                     ("kas2_bldg_platform_1.msh", placed_at(0, 18, 0)), ("yav_bldg_tower_turret", placed_at(5, 0, 5)),
                     ("yav_prop_mist", placed_at(1, 1, 1))]
        objects = audit.placed_objects(instances, CLASSES, MESHES, SHAPES, {}, LIVE, SOLDIER_SHAPES, {})
        self.assertEqual([o["model"] for o in objects], ["yav_prop_leaftree1", "yav_prop_leaftree1", "kas2_bldg_platform_1", "yav_bldg_watchtower", "yav_prop_mist"])
        self.assertEqual([bool(o["vertices"]) for o in objects], [True, True, True, True, False])
        with tempfile.TemporaryDirectory() as tmp, patch("extract.collision_sources", return_value=(MESHES, SHAPES)):
            out = Path(tmp)
            # An extract made by the class-name lookup: every one of these is a gap.
            write_collision([b""], instances, None, out, {}, {}, LIVE)
            stale = audit.compare(objects, audit.read_vertices(out / "collision.bin"), audit.read_vertices(out / "props_collision.bin"))
            self.assertEqual((stale["placed"], stale["withCollision"], stale["present"]), (5, 4, 0))
            self.assertEqual({label: entry["count"] for label, entry in stale["missing"].items()},
                             {"yav_prop_leaftree": 2, "kas2_bldg_platform_1.msh": 1, "yav_bldg_tower_turret": 1})
            self.assertEqual(stale["missing"]["yav_prop_leaftree"]["model"], "yav_prop_leaftree1")
            self.assertTrue(stale["missing"]["yav_bldg_tower_turret"]["live"])
            self.assertEqual(stale["noRetailCollision"], {"yav_prop_mist": {"count": 1, "model": "yav_prop_mist", "base": ""}})
            self.assertIn("MISSING", audit.describe("yav1", {**stale, "extract": str(out)}))
            # The corrected extract holds all of it, the turret's in the props buffers.
            write_collision([b""], instances, None, out, {}, {}, LIVE, classes=CLASSES)
            write_props([{"class": "yav_bldg_tower_turret", "name": "tower", "xfrm": placed_at(5, 0, 5), "props": []}],
                        CLASSES, LIVE, {}, {}, [b""], out, {}, SOLDIER_SHAPES)
            fresh = audit.compare(objects, audit.read_vertices(out / "collision.bin"), audit.read_vertices(out / "props_collision.bin"))
            self.assertEqual((fresh["present"], fresh["missing"], fresh["partial"]), (4, {}, {}))


if __name__ == "__main__":
    unittest.main()
