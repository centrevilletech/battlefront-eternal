"""Tests for tools/zero_fp2.py using synthetic archives and invented names."""

import array
import json
from pathlib import Path
import struct
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))

from zero_hash import crc, fnv1a  # noqa: E402
import zero_fp2  # noqa: E402


def _chunk(tag, body):
    pad = (-len(body)) & 3
    return tag + struct.pack("<I", len(body)) + body + b"\x00" * pad


def _ucfb(*inner):
    body = b"".join(inner)
    return b"ucfb" + struct.pack("<I", len(body)) + body


def _cstr(text):
    return text.encode("ascii") + b"\x00"


def _make_prop(name, val):
    return _chunk(b"PROP", struct.pack("<I", fnv1a(name)) + _cstr(val))


def _make_entc(name, base, props):
    body = _chunk(b"TYPE", _cstr(name)) + _chunk(b"BASE", _cstr(base))
    for k, v in props:
        body += _make_prop(k, v)
    return _chunk(b"entc", body)


def _make_zaa(bank_name, clips_spec):
    """Build a minimal valid `zaa_` chunk with SMNA/MINA/TNJA/TADA."""
    # clips_spec: [(clip_name, flags, [(bone_name, [(qx,qy,qz,qw,tx,ty,tz), ...]), ...])]
    tada = bytearray()
    tnja = bytearray()
    mina = bytearray()
    total_bones = 0

    for clip_name, flags, bone_tracks in clips_spec:
        frames = len(bone_tracks[0][1])
        mina += struct.pack("<IIIHH", flags, 0, crc(clip_name), frames, len(bone_tracks))
        for bone_name, samples in bone_tracks:
            total_bones += 1
            rot_offs = []
            for comp in range(4):
                rot_offs.append(len(tada))
                prev = int(round(samples[0][comp] * 2047.0))
                tada += struct.pack("<h", prev)
                for f in range(1, frames):
                    cur = int(round(samples[f][comp] * 2047.0))
                    delta = cur - prev
                    if -126 <= delta <= 127:
                        tada += struct.pack("<b", delta)
                    else:
                        tada += struct.pack("<bh", -127, cur)
                    prev = cur
            # Constant translation via scale = 0, bias = sample[0] translation.
            bx, by, bz = samples[0][4:7]
            tnja += struct.pack(
                "<I4I3I4f",
                crc(bone_name),
                rot_offs[0], rot_offs[1], rot_offs[2], rot_offs[3],
                0, 0, 0,
                bx, by, bz, 0.0,
            )

    bin_body = (
        b"SMNA" + struct.pack("<5I", 0, 8, total_bones, len(tada), len(clips_spec))
        + b"MINA" + struct.pack("<I", len(mina)) + bytes(mina)
        + b"TNJA" + struct.pack("<I", len(tnja)) + bytes(tnja)
        + b"TADA" + struct.pack("<I", len(tada)) + bytes(tada)
    )
    return _chunk(b"zaa_", _chunk(b"NAME", _cstr(bank_name)) + _chunk(b"BIN_", bin_body))


def _make_zaf(bank_name, joints):
    """Build a minimal valid `zaf_` chunk (NAME + BIN_ with FFAZ/SREV/LEKS/TNOJ)."""
    # joints: [(name, parent_name_or_none, (tx,ty,tz), (qx,qy,qz,qw))]
    tnoj_chunks = bytearray()
    for name, parent, pos, rot in joints:
        bcrc = crc(name)
        pcrc = crc(parent) if parent else 0
        payload = struct.pack(
            "<I4f3f3I",
            bcrc,
            rot[0], rot[1], rot[2], rot[3],
            pos[0], pos[1], pos[2],
            pcrc, 0, 0,
        )
        tnoj_chunks += _chunk(b"TNOJ", payload)
    ffaz_body = _chunk(b"SREV", struct.pack("<I", 8)) + _chunk(b"LEKS", bytes(tnoj_chunks))
    return _chunk(b"zaf_", _chunk(b"NAME", _cstr(bank_name)) + _chunk(b"BIN_", _chunk(b"FFAZ", ffaz_body)))


class ZeroFp2Tests(unittest.TestCase):
    def test_requested_roster_distinguishes_geometry_from_an_absent_class(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "side").mkdir()
            (root / "side/reed.lvl").write_bytes(_ucfb(_chunk(b"modl", _chunk(b"NAME", b"spindle\0"))))
            self.assertEqual(zero_fp2._collect_classes_from_archives(root, {"reed": ["spindle"]}), {})
            with self.assertRaisesRegex(ValueError, "requested class or model missing"):
                zero_fp2._collect_classes_from_archives(root, {"reed": ["lost"]})
            with self.assertRaisesRegex(ValueError, "missing requested first-person side"):
                zero_fp2._collect_classes_from_archives(root, {"lost": ["spindle"]})

    def test_explicit_class_bank_precedes_the_default_rig_policy(self):
        self.assertEqual(zero_fp2._bank_for_class("soldier", "plain", [("FirstPersonAnimationBank", "FlaxSpindle")]), "flaxspindle")

    def test_rejects_bad_track_counts_frames_and_nonfinite_samples(self):
        samples = [(0., 0., 0., 1., 2., 3., 5.)] * 3
        data = _make_zaa("humanfp", [("weave", 0, [("shuttle", samples)])])
        for offset, fmt, value in ((data.index(b"SMNA")+20, "I", 70),
                                   (data.index(b"MINA")+20, "H", 0),
                                   (data.index(b"MINA")+20, "H", 5000),
                                   (data.index(b"TNJA")+40, "f", float("nan"))):
            broken = bytearray(data)
            struct.pack_into("<"+fmt, broken, offset, value)
            with self.subTest(value=value), tempfile.TemporaryDirectory() as temp:
                with self.assertRaises(ValueError):
                    zero_fp2._decode_fp_banks([_ucfb(broken)], Path(temp) / "keys")

    def test_checks_model_buffer_before_unpacking_and_rejects_unsafe_names(self):
        segm = _chunk(b"segm", _chunk(b"IBUF", struct.pack("<I", 2**28)))
        with self.assertRaisesRegex(ValueError, "geometry buffer"):
            zero_fp2.check_mesh_buffers(_ucfb(_chunk(b"modl", segm)))
        with tempfile.TemporaryDirectory() as temp:
            for spec in ("../x;part", "a/b;../part", "/outside;part"):
                with self.subTest(spec=spec), self.assertRaises(ValueError):
                    zero_fp2.resolve_fpm_archive(Path(temp), spec)

    def test_writer_uses_little_endian_words(self):
        self.assertEqual(zero_fp2.little_endian(array.array("I", [0x12345678])), bytes.fromhex("78563412"))
        self.assertEqual(zero_fp2.little_endian(array.array("f", [1.25])), bytes.fromhex("0000a03f"))

    def test_topological_sort_reorders_child_before_parent(self):
        # Child appears at index 1 while its parent appears at index 2 (both name and index forms).
        ident = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]
        for parents in ([-1, 2, 0, 1], ["", "pulley", "mast", "jib"]):
            skel = {
                "names": ["mast", "jib", "pulley", "sheave"],
                "parents": parents,
                "local": [ident] * 4,
                "model": [ident] * 4,
            }
            bones, remap, _ = zero_fp2._topological_skeleton(skel)
            names = [b["name"] for b in bones]
            self.assertEqual(names, ["mast", "pulley", "jib", "sheave"])
            for idx, bone in enumerate(bones):
                self.assertLess(bone["parent"], idx)
            self.assertEqual(bones[remap[1]]["parent"], remap[2])

    def test_topological_sort_rejects_cycles(self):
        ident = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]
        skel = {
            "names": ["bone_a", "bone_b"],
            "parents": [1, 0],
            "local": [ident, ident],
            "model": [ident, ident],
        }
        with self.assertRaises(ValueError):
            zero_fp2._topological_skeleton(skel)

    def test_mixed_triangle_retains_each_corners_joint_in_topological_order(self):
        ident = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]
        shifted = ident.copy()
        shifted[12] = 3
        skel = {"names": ["jib", "mast"], "parents": [1, -1],
                "local": [shifted, ident], "model": [shifted, ident]}
        part = {"skin": [0, 1, 0], "bmap": [1, 0], "indices": [0, 1, 2],
                "positions": [(4, 1, 2), (5, 2, 7), (3, 0, 1)], "normals": [(0, 0, 1)] * 3,
                "uvs": [(0, 0), (1, 0), (0, 1)]}
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            zero_fp2._write_rig("crane", [part], skel, out)
            record = json.loads((out / "crane.json").read_bytes())["parts"][0]
            self.assertEqual(record["joints"], [0, 1, 0])
            self.assertEqual(struct.unpack_from("<3f", (out / "crane.bin").read_bytes(), 32), (2, 2, 7))
            self.assertEqual(struct.unpack("<3I", (out / "crane_index.bin").read_bytes()), (0, 1, 2))
            part["skin"][1] = 255
            with self.assertRaisesRegex(ValueError, "palette slot"):
                zero_fp2._write_rig("broken", [part], skel, out)

    def test_duplicate_bone_tracks_are_deduplicated_in_fp_bank(self):
        mast_offset = (0.37, -1.18, 0.64)
        grip_offset = (-0.52, 0.29, 1.43)
        sample = [(0.0, 0.0, 0.0, 1.0, *mast_offset)] * 2
        grip = [(0.0, 0.0, 0.0, 1.0, *grip_offset)] * 2
        zaa = _make_zaa(
            "humanfp",
            [
                ("lantern_arc", 8, [("mast", sample), ("mast", sample), ("hp_weapons", grip)]),
            ],
        )
        zaf = _make_zaf(
            "humanfp",
            [
                ("mast", None, mast_offset, (0.0, 0.0, 0.0, 1.0)),
                ("hp_camera", "mast", (3.25, -5.5, 7.125), (0.0, 0.0, 0.0, 1.0)),
            ],
        )
        archive = _ucfb(zaa, zaf)
        with tempfile.TemporaryDirectory() as tmp:
            anims_bin = Path(tmp) / "anims.bin"
            banks = zero_fp2._decode_fp_banks([archive], anims_bin)
            self.assertIn("humanfp", banks)
            clip = banks["humanfp"]["clips"][f"{crc('lantern_arc'):08x}"]
            self.assertEqual([t["name"] for t in clip["tracks"]], [f"{crc('mast'):08x}", "hp_weapons"])
            self.assertEqual(clip["frames"], 2)
            # 2 tracks * 2 frames * 7 floats * 4 bytes = 112 bytes
            self.assertEqual(anims_bin.stat().st_size, 2 * 2 * 7 * 4)
            keys = struct.unpack("<28f", anims_bin.read_bytes())
            for frame in range(2):
                for axis in range(3):
                    self.assertAlmostEqual(keys[frame * 7 + 4 + axis], mast_offset[axis], places=6)
                    self.assertAlmostEqual(keys[14 + frame * 7 + 4 + axis], grip_offset[axis], places=6)
            skeleton = banks["humanfp"]["skeleton"]
            for got, want in zip(skeleton["joints"][0]["position"], mast_offset):
                self.assertAlmostEqual(got, want, places=6)
            self.assertEqual(skeleton["cameraOffset"], [3.25, -5.5, 7.125])

    def test_lua50_globals_decodes_camera_motions(self):
        source = Path(__file__).parent / "fixtures/camera-motions/pendulum.luac"
        common = _ucfb(_chunk(b"scr_", _chunk(b"NAME", _cstr("globals")) + _chunk(b"BODY", source.read_bytes())))
        motion = zero_fp2.extract_camera_motions(common)["Land"]
        self.assertEqual(motion["Time"], [0.375, 0.625, 1.125])
        self.assertEqual(motion["Vec"][1], [8, 0, -2])
        self.assertEqual(motion["Ore"][0], motion["Ore"][2])
        self.assertEqual(motion["Slope"][0], motion["RSlope"][2])

    def test_classify_unit_rules(self):
        ranged, reason = zero_fp2._classify_unit({
            "base": "soldier",
            "parents": ["base_infantry"],
            "props": [("FirstPerson", "alpha\\rig1;alpha_1st_arm"), ("FirstPersonFOV", "72"), ("AimTension", "12")],
        })
        self.assertIsNone(reason)
        self.assertEqual(ranged["fov"], 72.0)
        self.assertEqual(ranged["aimTension"], 12.0)

        jedi, reason = zero_fp2._classify_unit({
            "base": "soldier",
            "parents": ["com_jedi_default"],
            "props": [("FirstPerson", "alpha\\rig1;alpha_1st_arm")],
        })
        self.assertIsNone(jedi)
        self.assertEqual(reason, "jedi")

        forced, reason = zero_fp2._classify_unit({
            "base": "soldier",
            "parents": ["base_infantry"],
            "props": [("FirstPerson", "alpha\\rig1;alpha_1st_arm"), ("ForceMode", "1")],
        })
        self.assertIsNone(forced)
        self.assertEqual(reason, "force_mode")

        absent, reason = zero_fp2._classify_unit({
            "base": "soldier",
            "parents": ["base_hero"],
            "props": [("FirstPersonFOV", "66")],
        })
        self.assertIsNone(absent)
        self.assertEqual(reason, "no_first_person")

    def test_write_rig_and_bounds_check(self):
        ident = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]
        skel = {
            "names": ["mast", "hp_weapons"],
            "parents": [-1, 0],
            "local": [ident, ident],
            "model": [ident, ident],
        }
        part = {
            "bone": "mast",
            "local_positions": [(0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0)],
            "local_normals": [(0.0, 0.0, 1.0)] * 3,
            "uvs": [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)],
            "indices": [0, 1, 2],
            "texture": "synth_tex",
            "bump": "synth_bump",
            "material": {"flags": 33, "specular": [10.0, 1.0, 1.0, 1.0]},
            "additive": False,
            "transparent": False,
        }
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "rigs"
            entry, used = zero_fp2._write_rig("synth_1st_rig", [part], skel, out)
            self.assertEqual(entry["bones"], 2)
            self.assertEqual(entry["parts"], 1)
            self.assertEqual(used, {"synth_tex", "synth_bump"})
            doc = json.loads((out / "synth_1st_rig.json").read_text())
            self.assertEqual(doc["stride"], 32)
            self.assertEqual((out / "synth_1st_rig.bin").stat().st_size, 3 * 32)
            self.assertEqual((out / "synth_1st_rig_index.bin").stat().st_size, 3 * 4)

            bad = dict(part, indices=[0, 1, 9])
            with self.assertRaises(ValueError):
                zero_fp2._write_rig("bad_rig", [bad], skel, out)


if __name__ == "__main__":
    unittest.main()
