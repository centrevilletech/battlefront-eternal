import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from extract import animated_prop_animation, placed_attachments, rig_parts, upright_xfrm
from xform import IDENTITY, mat4, mul


def chunk(tag, body):
    return tag + struct.pack("<I", len(body)) + body + b"\0" * (-len(body) % 4)


def soft_model(name, records, bmap, weights=None):
    """A model of one triangle with a soft skin (SKIN type 3: two weights
    out of 255 and three BMAP slots a vertex), its vertices in the model's
    space; with `weights`, its vertex buffer carries each vertex's first two
    weights as floats, as the retail tentacle's does."""
    positions = [(0.0, 0.0, 0.0), (0.0, 1.0, 0.0), (1.0, 0.0, 0.0)]
    if weights:
        flags = 0b10 | 0b1000 | 0b100 | 0b100000 | 0b1000000000  # POS, BONE_W, BONE_IDX, NORMAL, UV
        vertices = b"".join(struct.pack("<3f2f4B3f2f", *pos, *w, 0, 0, 0, 0, 0.0, 1.0, 0.0, 0.0, 0.0) for pos, w in zip(positions, weights))
        stride = 44
    else:
        flags = 0b10 | 0b100000 | 0b1000000000  # POS, NORMAL, UV
        vertices = b"".join(struct.pack("<3f3f2f", *pos, 0.0, 1.0, 0.0, 0.0, 0.0) for pos in positions)
        stride = 32
    segment = (chunk(b"INFO", struct.pack("<III", 4, 3, 1)) + chunk(b"TNAM", struct.pack("<I", 0) + name.encode() + b"\0")
               + chunk(b"IBUF", struct.pack("<I3H", 3, 0, 1, 2)) + chunk(b"VBUF", struct.pack("<III", 3, stride, flags) + vertices)
               + chunk(b"SKIN", struct.pack("<II", 3, 3) + bytes(b for record in records for b in record))
               + chunk(b"BMAP", struct.pack("<I", len(bmap)) + bytes(bmap)))
    info = struct.pack("<3I12f", 0, 0, 0, *([-1.0] * 3 + [1.0] * 3 + [0.0] * 3 + [1.0] * 3))
    return chunk(b"modl", chunk(b"NAME", name.encode() + b"\0") + chunk(b"INFO", info) + chunk(b"segm", segment))


class SoftSkinParts(unittest.TestCase):
    """The sarlacc tentacle is one soft-skinned segment over nine bones.
    Split by each triangle's first bone, its pieces came metres apart at
    every bend; it keeps its three bones and weights a vertex instead."""

    def setUp(self):
        moved = [1.0, 0, 0, 0, 0, 1.0, 0, 0, 0, 0, 1.0, 0, 5.0, 5.0, 5.0, 1.0]
        self.skeleton = {"names": ["root", "bone_a", "bone_b"], "parents": [None, "root", "root"], "local": [moved] * 3, "model": [moved] * 3}

    def parse(self, weights=None):
        from zero_model import parse_model
        data = soft_model("tentacle", [(255, 0, 1, 0, 0), (100, 50, 0, 1, 2), (0, 200, 0, 1, 2)], [2, 1, 0], weights)
        return parse_model(data, 8, len(data), {"tentacle": self.skeleton})[1]

    def test_each_vertex_keeps_its_three_bones_and_weights(self):
        part = self.parse()[0]
        self.assertEqual(part["joints"], [1, 0, 1], "the heaviest, as before")
        self.assertEqual(part["skinJoints"], [1, 2, 2, 2, 1, 0, 2, 1, 0], "every slot through BMAP")
        self.assertEqual([round(w, 4) for w in part["skinWeights"][3:6]], [round(100 / 255, 4), round(50 / 255, 4), round(105 / 255, 4)])

    def test_the_vertex_buffer_weights_are_the_exact_ones(self):
        part = self.parse(weights=[(1.0, 0.0), (0.4, 0.2), (0.0, 0.79)])[0]
        self.assertEqual([round(w, 4) for w in part["skinWeights"][3:6]], [0.4, 0.2, 0.4], "the third is what the two leave")
        self.assertEqual([round(w, 4) for w in part["skinWeights"][6:9]], [0.0, 0.79, 0.21])

    def test_a_soft_part_stays_whole_in_the_models_space(self):
        rigged = rig_parts(self.parse(), self.skeleton)
        self.assertEqual(len(rigged), 1, "not split by bone")
        self.assertIsNone(rigged[0]["bone"])
        self.assertEqual(rigged[0]["local_positions"][1], (0.0, 1.0, 0.0), "where it is, not carried into a bone's space")
        self.assertEqual(len(rigged[0]["skinJoints"]), 9)


class PlacedAttachments(unittest.TestCase):
    def setUp(self):
        self.raw = [1, 0, 0, 0, 1, 0, 0, 0, 1, 10, 20, 30]
        self.records = [{"class": "bed", "xfrm": self.raw}]
        self.classes = {
            "bed": {"base": "prop", "props": [
                ("GeometryName", "bedframe"),
                ("AttachOdf", "jabba"), ("AttachToHardPoint", "hp_jabba"),
                ("AttachOdf", "crown"), ("AttachToHardPoint", "hp_top"),
                ("AttachOdf", "lamp"), ("AttachToHardPoint", "hp_top"),
                ("AttachOdf", "ghost"), ("AttachToHardPoint", "hp_top"),
            ]},
            "jabba": {"base": "animatedprop", "props": [("GeometryName", "jabba_body")]},
            "crown": {"base": "leafpatch", "props": []},
            "lamp": {"base": "light", "props": []},
            "ghost": {"base": "animatedprop", "props": [("GeometryName", "no_such_model")]},
        }
        jabba_local = list(IDENTITY)
        jabba_local[12:15] = [1, 2, 3]
        self.skeletons = {"bedframe": {"names": ["hp_jabba", "hp_top"], "model": [jabba_local, list(IDENTITY)]}}
        self.models = {"jabba_body": [{"texture": "jabba"}]}

    def test_geometry_attachment_becomes_an_instance_at_the_hardpoint(self):
        found = placed_attachments(self.records, self.classes, self.skeletons, self.models)
        self.assertEqual([kind for kind, _ in found], ["jabba"],
                         "foliage, lights and model-less attachments stay out")
        matrix = mat4(found[0][1])
        self.assertEqual(matrix[12:15], [11, 22, 33])

    def test_attached_prop_stands_upright_at_a_tipped_hardpoint(self):
        # hp_rancor's basis: Y mostly forward. Uprighting keeps the spot
        # and the facing (forward +X here), with Y straight up.
        m = [0.0, 0.924, -0.383, 0.0, 0.0, 0.383, 0.924, 0.0, 1.0, 0.0, 0.0, 0.0, -41.27, -17.8, -0.57, 1.0]
        raw = upright_xfrm(m)
        self.assertEqual(raw[3:6], [0.0, 1.0, 0.0])
        self.assertEqual(raw[6:9], [1.0, 0.0, 0.0])
        self.assertEqual(raw[9:12], [-41.27, -17.8, -0.57])
        # A yaw-only hardpoint (Jabba's bed) passes through unchanged.
        yaw = mat4([0, 0, -1, 0, 1, 0, 1, 0, 0, 5, 6, 7])
        self.assertEqual(upright_xfrm(yaw), [0, 0, -1, 0, 1, 0, 1, 0, 0, 5, 6, 7])

    def test_animated_prop_animation_parsing(self):
        jabba = {"base": "animatedprop", "props": [("Animation", "idle"), ("Animation", "grab"), ("AnimationTrigger", "grab hp_hotspot 3.0")]}
        trigger = {"action": "grab", "node": "hp_hotspot", "radius": 3.0}
        self.assertEqual(animated_prop_animation(jabba),
                         {"idle": "idle", "clips": ["idle", "grab"], "trigger": trigger, "triggers": [trigger]})
        trap = {"base": "animatedprop", "props": [("Animation", "idle"), ("AnimationTrigger", "open hp_trapradius 1.0 trap_open"), ("OpenSound", "trap_door_open")]}
        parsed = animated_prop_animation(trap)
        self.assertEqual(parsed["trigger"], {"action": "open", "node": "hp_trapradius", "radius": 1.0, "sound": "trap_open"})
        self.assertEqual(parsed["openSound"], "trap_door_open")
        door = {"base": "door", "props": [("Animation", "open"), ("AnimationTrigger", "hp_doorradius 2.0"), ("CloseSound", "door_close")]}
        self.assertEqual(animated_prop_animation(door)["trigger"], {"action": "open", "node": "hp_doorradius", "radius": 2.0})
        self.assertIsNone(animated_prop_animation({"base": "animatedprop", "props": []}))

    def test_the_sarlacc_tentacle_keeps_every_trigger_and_grab(self):
        """tat1_bldg_sarlacctentacle as Tatooine: Dune Sea's archive has it:
        four clips, three hotspots, three grabs (AttachTrigger, hash
        72306628), its swallow's sound and the camera flag."""
        tentacle = {"base": "animatedprop", "props": [
            ("AnimationName", "tat1_bldg_sarlacctentacle"), ("Animation", "idle"), ("Animation", "grab"), ("Animation", "grab1"), ("Animation", "grab2"),
            ("AnimationTrigger", "grab hp_hotspot 5.0"), ("AnimationTrigger", "grab1 hp_hotspot1 5.0"), ("AnimationTrigger", "grab2 hp_hotspot2 5.0"),
            ("AttachTrigger", "hp_active 1.5 grab human_rifle_thrown_flail 2.5 hp_camera die"),
            ("AttachTrigger", "hp_active 1.75 grab1 human_rifle_thrown_flail 2.5 hp_camera die"),
            ("AttachTrigger", "hp_active 2.0 grab2 human_rifle_thrown_flail 2.5 hp_camera die"),
            ("KillSoldierSound", "sarlacc_belch"), ("6d8fe606", "1"), ("0550a3b2", "1")]}
        parsed = animated_prop_animation(tentacle)
        self.assertEqual(parsed["clips"], ["idle", "grab", "grab1", "grab2"])
        self.assertEqual([(t["action"], t["node"], t["radius"]) for t in parsed["triggers"]],
                         [("grab", "hp_hotspot", 5.0), ("grab1", "hp_hotspot1", 5.0), ("grab2", "hp_hotspot2", 5.0)])
        self.assertEqual(parsed["attach"], [
            {"node": "hp_active", "radius": radius, "action": action, "clip": "human_rifle_thrown_flail", "window": 2.5}
            for radius, action in ((1.5, "grab"), (1.75, "grab1"), (2.0, "grab2"))])
        self.assertEqual(parsed["killSound"], "sarlacc_belch")
        self.assertTrue(parsed["deathCamera"])
        self.assertNotIn("fromStart", parsed)
        forklift = {"base": "animatedprop", "props": [("Animation", "basepose"), ("Animation", "activate"),
                    ("AnimationTrigger", "activate hp_activate 0.5"), ("6b014234", "1"), ("IdleDelay", "10.0")]}
        parsed = animated_prop_animation(forklift)
        self.assertEqual((parsed["idleDelay"], parsed["fromStart"]), (10.0, True))

    def test_missing_hardpoint_or_skeleton_is_skipped(self):
        self.skeletons = {"bedframe": {"names": ["hp_top"], "model": [list(IDENTITY)]}}
        self.assertEqual(placed_attachments(self.records, self.classes, self.skeletons, self.models), [])
        self.assertEqual(placed_attachments(self.records, self.classes, {}, self.models), [])


if __name__ == "__main__":
    unittest.main()
