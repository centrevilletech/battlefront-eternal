"""Battlefront II particle archive and attachment selection with invented assets."""

from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
import json
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest.mock import patch
import sys
import struct
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
import zero_fx2 as fx
from xform import IDENTITY
from zero_hash import fnv1a


def odf(name, base, *pairs):
    return {"name": name, "base": base, "props": pairs}


class EffectBindings(unittest.TestCase):
    def test_only_reachable_slots_inherit_and_keep_repeated_bindings(self):
        catalog = {
            "entc": {"vane": odf("vane", "prop", ("WeaponName1", "Coil"))},
            "wpnc": {"coil": odf("coil", "spring", ("AddHitEffect", "shimmer"), ("OrdnanceName", "bead")),
                     "spring": odf("spring", "cannon", ("AddHitEffect", "ring")),
                     "unused": odf("unused", "cannon", ("MuzzleFlashEffect", "wrong"))},
            "ordc": {"bead": odf("bead", "shell", ("ExplosionName", "haze"), ("LaserTexture", "woven"))},
            "expc": {"haze": odf("haze", "explosion", ("Effect", "vapor"), ("aa00bb11", "ripple"))},
        }
        bound, _, textures = fx.bindings(catalog, [("entc", "VANE")], {fnv1a("ripple"): True})
        self.assertEqual(bound, {"wpnc/coil": {"AddHitEffect": ["ring", "shimmer"]},
                                 "expc/haze": {"Effect": ["vapor"], "aa00bb11": ["ripple"]}})
        self.assertEqual(textures, {"woven"})

    def test_a_hashed_critical_explosion_is_followed_to_its_effect(self):
        catalog = {
            "entc": {"dart": odf("dart", "flyer", (f"{fnv1a('ExplosionCritical'):08x}", "flare"),
                                 ("ExplosionDestruct", "puff"))},
            "wpnc": {}, "ordc": {},
            "expc": {"flare": odf("flare", "explosion", ("Effect", "bloom")),
                     "puff": odf("puff", "explosion", ("Effect", "smudge"))},
        }
        links = {}
        bound, _, _ = fx.bindings(catalog, [("entc", "dart")], {}, links)
        self.assertEqual(links["entc/dart"], {"ExplosionCritical": ["expc/flare"], "ExplosionDestruct": ["expc/puff"]})
        self.assertEqual(bound["expc/flare"], {"Effect": ["bloom"]})
        self.assertEqual(bound["expc/puff"], {"Effect": ["smudge"]})

    def test_attachment_uses_its_joint_and_reports_an_absent_joint(self):
        props = [("GeometryName", "Spindle"), ("AttachEffect", "wisp"), ("AttachToHardPoint", "Tip"),
                 ("AttachToHardPoint", "Foot")]
        joint = list(IDENTITY)
        joint[13] = 3.25
        placed, missing = fx.placements(
            [{"class": "Lamp", "name": "west", "props": [], "xfrm": (1, 0, 0, 0, 1, 0, 0, 0, 1, 2, 8, 5)}],
            {"lamp": props}, {"spindle": {"names": ["tip"], "model": [joint]}})
        self.assertEqual(placed[0]["effect"], "wisp")
        self.assertEqual(placed[0]["matrix"][12:15], [2, 11.25, 5])
        self.assertEqual(missing, [("lamp", "wisp", "Foot")])

    def test_mounted_round_inherits_its_visual_settings(self):
        catalog = {"entc": {}, "expc": {}, "wpnc": {"spout": odf("spout", "cannon", ("OrdnanceName", "pip"))},
                   "ordc": {"pip": odf("pip", "seed", ("LaserWidth", "0.37")),
                            "seed": odf("seed", "bolt", ("LightColor", "8 47 95"), ("Damage", "29"))}}
        rounds, links = {}, {}
        fx.bindings(catalog, [("wpnc", "spout")], {}, links, rounds)
        self.assertEqual(links["wpnc/spout"]["OrdnanceName"], ["ordc/pip"])
        self.assertEqual(rounds["ordc/pip"], {"base": "bolt", "LaserWidth": 0.37, "LightColor": [8, 47, 95]})

    def test_the_glow_texture_goes_with_a_bolt_or_a_beam_and_an_older_pack_is_prepared_again(self):
        self.assertEqual(fx.glow_textures({}), set())
        self.assertEqual(fx.glow_textures({"ordc/spark": {"base": "grenade"}}), set())
        for base in ("bolt", "beam", "bullet"):
            self.assertEqual(fx.glow_textures({"ordc/a": {"base": base}}), {fx.GLOW_TEXTURE})
        with TemporaryDirectory() as tmp:
            out = Path(tmp)
            pack = {"version": fx.PACK_VERSION, "hdr": None, **{k: {} for k in
                    ("effects", "models", "textures", "bindings", "mounts", "links", "beams")},
                    "rounds": {"ordc/a": {"base": "beam"}}, "missing": {"textures": []}}
            (out / "effects.json").write_text(json.dumps(pack))
            self.assertFalse(fx.pack_current(out), "a pack with a beam and no glow texture is prepared again")
            (out / "glow.png").write_bytes(b"x")
            with_glow = {**pack, "textures": {fx.GLOW_TEXTURE: "glow.png"}}
            (out / "effects.json").write_text(json.dumps(with_glow))
            self.assertTrue(fx.pack_current(out))
            # A game without the texture is not asked again every time.
            gone = {**pack, "missing": {"textures": [fx.GLOW_TEXTURE]}}
            (out / "effects.json").write_text(json.dumps(gone))
            self.assertTrue(fx.pack_current(out))
            # A pack of rounds that draw no glow needs none.
            plain = {**pack, "rounds": {"ordc/a": {"base": "grenade"}}}
            (out / "effects.json").write_text(json.dumps(plain))
            self.assertTrue(fx.pack_current(out))

    def test_secondary_beam_tracer_exports_range_and_links(self):
        catalog = {
            "entc": {}, "expc": {},
            "wpnc": {"rotary": odf("rotary", "cannon", ("OrdnanceName", "slug"), ("SecondaryOrdnanceName", "needle"))},
            "ordc": {
                "slug": odf("slug", "bolt", ("Velocity", "310"), ("LifeSpan", "1.2")),
                "needle": odf("needle", "beam", ("Range", "175"), ("LifeSpan", "0.12"),
                              ("LaserWidth", "0.07"), ("LaserTexture", "amber"), ("FadeOutTime", "0.15")),
            },
        }
        rounds, links = {}, {}
        _, _, textures = fx.bindings(catalog, [("wpnc", "rotary")], {}, links, rounds)
        self.assertEqual(links["wpnc/rotary"]["SecondaryOrdnanceName"], ["ordc/needle"])
        self.assertEqual(rounds["ordc/needle"], {
            "base": "beam", "Range": 175.0, "LifeSpan": 0.12,
            "LaserWidth": 0.07, "LaserTexture": "amber", "FadeOutTime": 0.15,
        })
        self.assertIn("amber", textures)

    def test_independent_damage_bands_and_contrail_offsets(self):
        props = [("DamageStartPercent", "63"), ("DamageEffect", "vapor"), ("DamageStopPercent", "13"),
                 ("ContrailEffect", "ribbon"), ("ContrailAttachOffset", "-3 0 2"), ("ContrailEffectMinSpeed", "9"),
                 ("DamageStartPercent", "12"), ("DamageEffect", "glint"), ("DamageAttachPoint", "absent")]
        mounts, missing = fx.mounting_points({"kite": props}, {})
        band, trail = mounts["kite"]["mounts"]
        self.assertEqual((band["StartPercent"], band["StopPercent"]), (63., 13.))
        self.assertEqual((trail["matrix"][12:15], trail["EffectMinSpeed"]), ([-3., 0., 2.], 9.))
        self.assertEqual(missing, {("kite", "glint", "absent")})

    def test_light_beam_uses_both_widths_and_explicit_distance(self):
        beam, = fx.light_beams([("LightBeam", [], [
            ("InitialWidth", [2.75], []), ("FinalWidth", [1.5, 8.25], []),
            ("DrawDistance", [43.], []), ("Color", [4., 21., 9.], [])])])
        self.assertEqual(beam["start"], [2.75, 2.75])
        self.assertEqual(beam["end"], [1.5, 8.25])
        self.assertEqual(beam["distance"], 43.)
        self.assertEqual(beam["color"], [4., 21., 9., 255.])

    def test_blur_settings_stay_on_the_nested_material(self):
        glass = ("ParticleEmitter", ["glass"], [("Geometry", [], [
            ("BlendMode", ["BLUR"], []), ("BlurRes", [0.], []), ("BlurValue", [0.37], [])])])
        tree = [("ParticleEmitter", ["carrier"], [("Geometry", [], [("Type", ["EMITTER"], []), glass])])]
        groups = fx.emitters(tree)
        fx.blur_settings(tree, groups)
        inner = groups[0]["geometry"]["emitters"][0]["geometry"]
        self.assertEqual((inner["blend"], inner["blurValue"], inner["blurRes"]), ("blur", 0.37, 0.))

    def test_camera_dust_comes_from_an_enabled_world_block(self):
        fields = [("Enable", [0], []), ("Texture", ["Ash"], []), ("SpawnDistance", [17.5], [])]
        tree = [("Effect", ["SpaceDust"], fields)]
        self.assertIsNone(fx.space_dust(tree))
        fields[0] = ("Enable", [1], [])
        self.assertEqual(fx.space_dust(tree), {"Texture": "ash", "SpawnDistance": 17.5})

    def test_world_bloom_keeps_its_fields_and_fills_the_rest_with_the_games(self):
        self.assertIsNone(fx.hdr_bloom([("Effect", ["Glare"], [("Enable", [1], [])])]))
        fields = [("Enable", [0], []), ("GlowThreshold", [0.45], []), ("NumBloomPasses", [7], [])]
        self.assertEqual(fx.hdr_bloom([("Effect", ["Hdr"], fields)]), {
            "DownSizeFactor": 0.25, "NumBloomPasses": 7.0, "MaxTotalWeight": 1.2, "GlowThreshold": 0.45, "GlowFactor": 1.0})

    def test_a_pack_without_the_world_bloom_is_prepared_again(self):
        with TemporaryDirectory() as tmp:
            out = Path(tmp)
            pack = {"version": fx.PACK_VERSION, **{k: {} for k in
                    ("effects", "models", "textures", "bindings", "mounts", "links", "rounds", "beams")}}
            (out / "effects.json").write_text(json.dumps(pack))
            self.assertFalse(fx.pack_current(out))
            (out / "effects.json").write_text(json.dumps({**pack, "hdr": None}))
            self.assertTrue(fx.pack_current(out))

    def test_particle_budget_counts_both_bindings_and_placed_nested_emitters(self):
        def emitter(limit, children):
            return {"maxParticles": [limit, limit], "burstDelay": [0.25, 1.], "burstCount": [1., 3.],
                    "transformer": {"lifetime": 2.}, "geometry": {"emitters": children}}
        library = {"pip": [emitter(5., [emitter(2., [])])]}
        self.assertEqual(fx.particle_budget(library, {"stem": {"Effect": ["pip"]}}, [{"effect": "pip"}]), 32)
        self.assertEqual(fx.particle_budget(library, {}, []), 1)

    def test_malformed_resource_is_reported_and_its_siblings_are_published(self):
        names = ("hollow", "cracked", "blank", "whole")
        nodes = {fnv1a(n): (n, 0, 0) for n in names}
        def config(name, *_):
            radius = {"hollow": ["bad"], "cracked": [float("inf")], "blank": [], "whole": [2.5]}[name]
            return [("ParticleEmitter", ["coil"], [("BurstCount", radius, [])])]
        bound = {"fixture": {"Effect": list(names)}}
        with TemporaryDirectory() as tmp, patch.object(fx, "sources", return_value=[(b"", [])]), \
                patch.object(fx, "resources", return_value=(nodes, {}, {}, {})), \
                patch.object(fx, "bindings", return_value=(bound, {}, set())), patch.object(fx, "parse", config):
            errors, output = StringIO(), StringIO()
            with redirect_stderr(errors), redirect_stdout(output):
                fx.write_pack(None, Path(tmp), "pond", {"script": "wind", "calls": []})
            pack = json.loads((Path(tmp) / "maps/pond/effects/wind/effects.json").read_bytes())
        self.assertEqual(list(pack["effects"]), ["whole"])
        self.assertEqual(pack["missing"]["effects"], ["blank", "cracked", "hollow"])
        self.assertTrue(all(row["reason"] and row["resource"] in errors.getvalue() for row in pack["missing"]["invalid"]))
        self.assertIn("3 invalid effect resources skipped", output.getvalue())

    def test_bad_mount_does_not_remove_another_mount_of_the_same_class(self):
        skipped = []
        props = [("DamageStartPercent", "nan"), ("DamageEffect", "grain"), ("ThrustEffect", "mist")]
        with redirect_stderr(StringIO()):
            mounts, _ = fx.mounting_points({"fan": props}, {}, skipped)
        self.assertEqual([m["effect"] for m in mounts["fan"]["mounts"]], ["mist"])
        self.assertEqual(len(skipped), 1)

    def test_invalid_chunks_and_nesting_are_isolated_by_name(self):
        def chunk(tag, body):
            return tag + struct.pack("<I", len(body)) + body + bytes(-len(body) % 4)

        def nested(levels):
            field = chunk(b"DATA", struct.pack("<IBf", fnv1a("Fold"), 1, 1.25))
            branch = field
            for _ in range(levels):
                branch = field + chunk(b"SCOP", branch)
            return branch

        records = {"fine": nested(2), "hundred": nested(100), "fourhundred": nested(400),
                   "overrun": chunk(b"SCOP", b"DATA" + struct.pack("<I", 65536)),
                   "wide": chunk(b"DATA", struct.pack("<IBf", fnv1a("Fold"), 255, 2.5)),
                   "extension": nested(fx.MAX_EFFECT_DEPTH - 1)}
        nodes = {fnv1a(name): (data, 0, len(data)) for name, data in records.items()}
        with TemporaryDirectory() as tmp, patch.object(fx, "sources", return_value=[(b"", [])]), \
                patch.object(fx, "resources", return_value=(nodes, {}, {}, {})), \
                patch.object(fx, "bindings", return_value=({"stem": {"Effect": list(records)}}, {}, set())):
            errors = StringIO()
            with redirect_stdout(StringIO()), redirect_stderr(errors):
                fx.write_pack(None, Path(tmp), "pond", {"script": "wind", "calls": []})
            pack = json.loads((Path(tmp) / "maps/pond/effects/wind/effects.json").read_bytes())
        self.assertEqual(list(pack["effects"]), ["fine"])
        reasons = {row["resource"]: row["reason"] for row in pack["missing"]["invalid"]}
        for name, why in (("hundred", "scope nesting"), ("fourhundred", "scope nesting"),
                          ("extension", "prepared effect nesting"), ("overrun", "truncated DATA"),
                          ("wide", "value count 255")):
            self.assertIn(why, reasons[name])
            self.assertIn(name, errors.getvalue())

    def test_cyclic_class_contributes_no_effects(self):
        catalog = {"ordc": {"twist": odf("twist", "twist", ("TrailEffect", "loop")),
                            "reed": odf("reed", "bolt", ("TrailEffect", "grain"))}}
        skipped = []
        with redirect_stderr(StringIO()):
            bound, _, _ = fx.bindings(catalog, [("ordc", "twist"), ("ordc", "reed")], {}, skipped=skipped)
        self.assertEqual(bound, {"ordc/reed": {"TrailEffect": ["grain"]}})
        self.assertEqual([row["resource"] for row in skipped], ["ordc/twist"])
        self.assertIn("cycle", skipped[0]["reason"])

    def test_muzzle_flash_and_discharge_are_not_particle_bindings(self):
        catalog = {
            "entc": {}, "expc": {}, "ordc": {},
            "wpnc": {
                "lance": odf(
                    "lance", "cannon",
                    ("MuzzleFlash", "preset_flash"),
                    ("Discharge", "preset_smoke"),
                    ("MuzzleFlashEffect", "spark"),
                ),
            },
        }
        flashes = {}
        bound, _, _ = fx.bindings(
            catalog, [("wpnc", "lance")],
            {fnv1a("preset_smoke"): True, fnv1a("spark"): True},
            flashes=flashes,
        )
        self.assertEqual(bound, {"wpnc/lance": {"MuzzleFlashEffect": ["spark"]}})
        self.assertEqual(flashes, {"wpnc/lance": {"MuzzleFlash": "preset_flash"}})


class PackVersion(unittest.TestCase):
    def test_the_client_reads_the_layout_version_the_tool_writes(self):
        reader = Path(__file__).resolve().parents[1] / "rust/bfe-content-native/src/effects/swbf2.rs"
        self.assertIn(f"const PACK_VERSION: f64 = {fx.PACK_VERSION}.;", reader.read_text(encoding="utf-8"))
