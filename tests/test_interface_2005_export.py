"""The 2005 interface tools (tools/zero_hud2.py, tools/zero_load2.py):
the strict config reader accounts for every byte or refuses the chunk,
the HUD trees dump as JSON, and the loading screen's display, worlds,
tips and picture sets come out of synthetic archives. The last class
parses a real install when BFE_SWBF2_DIR names one."""
import json
import os
import re
import struct
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from zero_hash import fnv1a  # noqa: E402
import zero_hud2  # noqa: E402
import zero_load2  # noqa: E402
from zero_hud2 import ConfigError  # noqa: E402


def chunk(tag, body):
    return tag + struct.pack("<I", len(body)) + body + b"\0" * (-len(body) % 4)


def data(name, *values):
    """A config DATA chunk. Values are floats, strings, or ("bits", word)
    for a raw word (a name hash)."""
    blob = bytearray()
    words = []
    for value in values:
        if isinstance(value, str):
            words.append(4 * len(values) + len(blob))
            blob += value.encode("latin1") + b"\0"
        elif isinstance(value, tuple):
            words.append(value[1])
        else:
            words.append(struct.unpack("<I", struct.pack("<f", float(value)))[0])
    body = struct.pack("<IB", fnv1a(name), len(values))
    body += b"".join(struct.pack("<I", word) for word in words)
    body += struct.pack("<I", len(blob)) + bytes(blob)
    return chunk(b"DATA", body)


def scope(*children):
    return chunk(b"SCOP", b"".join(children))


def name_chunk(name):
    return chunk(b"NAME", struct.pack("<I", fnv1a(name)))


def archive(*nodes):
    inner = b"".join(nodes)
    return b"ucfb" + struct.pack("<I", len(inner)) + inner


def config(tag, *body):
    return archive(chunk(tag, b"".join(body)))


def locl(texts, language="english"):
    body = b"".join(
        struct.pack("<IH", fnv1a(key), 6 + len(text.encode("utf-16-le")) + 2)
        + text.encode("utf-16-le") + b"\0\0"
        for key, text in texts.items())
    return archive(chunk(b"Locl", chunk(b"NAME", language.encode() + b"\0") + chunk(b"BODY", body)))


def small_font():
    # Unequal advances, including a very wide glyph.
    rows = b"".join(struct.pack("<H6B4f", c, 0, 30 if c == 87 else 5, 0, 5, 0, 16, 0, 1, 0, 1)
                    for c in range(32, 127))
    return chunk(b"font", chunk(b"NAME", b"gamefont_small\0")
                 + chunk(b"HEAD", struct.pack("<HBBB", 95, 1, 17, 13))
                 + chunk(b"FBOD", rows))


def texture(name, width=4, height=4):
    info = struct.pack("<IHHHHI", 0x31545844, width, height, 1, 1, 1)
    fmt = chunk(b"FMT_", chunk(b"INFO", info) + chunk(b"FACE", bytes(8 * ((width + 3) // 4) * ((height + 3) // 4))))
    return chunk(b"tex_", chunk(b"NAME", name.encode() + b"\0") + fmt)


class StrictConfigTests(unittest.TestCase):
    @unittest.skipUnless(os.environ.get("SWBF_UNMUNGE_NAMES"), "needs the upstream name list")
    def test_interface_vocabulary_sources(self):
        source = Path(os.environ["SWBF_UNMUNGE_NAMES"]).read_text()
        upstream = set(re.findall(r'"([^"]+)"_fnvp', source))
        local = set(zero_hud2.NAMES.values())
        normalized = {name.lower() for name in upstream}
        self.assertEqual(len(local & upstream), 172)
        self.assertEqual(sum(name.lower() in normalized for name in local - upstream), 20)
        self.assertEqual({name for name in local if name.lower() not in normalized}, {"LoadDisplay"})
        notices = (Path(__file__).resolve().parents[1] / "THIRD-PARTY-NOTICES.md").read_text()
        self.assertIn("Copyright (c) 2017-2019 PrismaticFlower", notices.splitlines())

    def test_tree_values_and_file_name(self):
        raw = config(b"hud_",
                     name_chunk("1playerhud"),
                     data("FileInfo", "1playerhud") + scope(data("Viewports", 1.0)),
                     data("Bitmap", "mask") + scope(
                         data("position", 0.5, -0.25, 0.0, "Viewport"),
                         data("ZOrder", 7.0),
                         data("EventEnable", "initialize")))
        found = zero_hud2.config_chunks(raw, b"hud_")
        self.assertEqual(len(found), 1)
        self.assertEqual(found[0]["file"], "1playerhud")
        entries = zero_hud2.to_jsonable(found[0]["entries"])
        self.assertEqual([e["name"] for e in entries], ["1playerhud", "FileInfo", "Bitmap"])
        self.assertEqual(entries[1]["children"][0]["values"], [1])
        bitmap = entries[2]
        self.assertEqual(bitmap["values"], ["mask"])
        self.assertEqual(bitmap["children"][0]["values"], [0.5, -0.25, 0, "Viewport"])
        counts = zero_hud2.summarize(found[0]["entries"])
        self.assertEqual(counts["elements"], {"FileInfo": 1, "Bitmap": 1})
        self.assertEqual(counts["bindings"], {"initialize": 1})

    def test_rejects_corrupt_envelopes_identities_padding_and_offsets(self):
        name = name_chunk("hudtransforms")
        hud = chunk(b"hud_", name)
        node = data("ZOrder", 1)
        shared_suffix = chunk(b"DATA", struct.pack("<IBIII", 1, 2, 8, 9, 4) + b"abc\0")
        deep = node
        for _ in range(80):
            deep = node + scope(deep)
        invalid = [
            archive(hud)[:-1], archive(hud) + b"x", archive(hud, hud),
            archive(chunk(b"lvl_", b"abc")), config(b"hud_", node),
            config(b"hud_", name, node[:-1] + b"Z"),
            config(b"hud_", name, chunk(b"DATA", b"a")),
            config(b"hud_", name, shared_suffix), config(b"hud_", name, deep),
            config(b"hud_", name, node + scope(name)),
        ]
        for n, raw in enumerate(invalid):
            with self.subTest(case=n), self.assertRaises(ConfigError):
                zero_hud2.config_chunks(raw, b"hud_")

    def test_failed_preparation_does_not_publish_an_empty_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = fake_install(tmp)
            out = Path(tmp) / "out"
            for raw in [archive()[:-1], archive(), config(b"load", name_chunk("other")),
                        config(b"load", name_chunk("load"), data("map", "x"))]:
                (root / "load/load.lvl").write_bytes(raw)
                with self.assertRaises(ConfigError):
                    zero_load2.write_loading_screen(root, out)
                self.assertFalse((out / "common/shell/load2.json").exists())
            (root / "ingame.lvl").write_bytes(archive())
            with self.assertRaises(ConfigError):
                zero_hud2.write_hud_files(root, out)

    def test_empty_scope_and_float_words_survive_json(self):
        raw = config(b"hud_", name_chunk("1playerhud"),
                     data("Backdrop") + scope(), data("Scale", 1.25e-8, -0.0))
        entries = zero_hud2.config_chunks(raw, b"hud_")[0]["entries"]
        tree = json.loads(json.dumps(zero_hud2.to_jsonable(entries)))
        self.assertEqual(tree[1]["children"], [])
        self.assertNotIn("children", tree[2])
        self.assertEqual(struct.pack("<2f", *tree[2]["values"]), struct.pack("<2f", 1.25e-8, -0.0))
        self.assertEqual(zero_hud2.summarize(entries)["elements"], {"Backdrop": 1})

    def test_zero_words_are_not_chunk_alignment(self):
        name = name_chunk("hudtransforms")
        for tail in [bytes(4), bytes(7), bytes(4) + data("Scale", 1.75)]:
            with self.subTest(size=len(tail)), self.assertRaises(ConfigError):
                zero_hud2.config_chunks(config(b"hud_", name, tail), b"hud_")

    def test_hud_command_reads_the_tree_once(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = fake_install(tmp)
            (root / "ingame.lvl").write_bytes(config(b"hud_", name_chunk("hudtransforms")))
            with patch.object(zero_hud2, "hud_files", wraps=zero_hud2.hud_files) as read:
                zero_hud2.main(["--game", tmp, "--out", str(Path(tmp) / "out")])
            read.assert_called_once_with(root)

    def test_hash_words_keep_their_bits(self):
        raw = config(b"load", name_chunk("load"),
                     data("TipsTime", 10.0, ("bits", 0x89ABCDEF)))
        values = zero_hud2.config_chunks(raw, b"load")[0]["entries"][1]["values"]
        self.assertEqual(values[0], 10.0)
        self.assertEqual(values[1], {"bits": "0x89abcdef", "name": None})

    def test_refuses_bytes_it_cannot_account_for(self):
        good = data("ZOrder", 1.0)
        # A string offset past the table, an unterminated string, table
        # bytes no value references, an unknown child, a nonzero tail.
        bad_offset = chunk(b"DATA", struct.pack("<IBI", fnv1a("Mesh"), 1, 99) + struct.pack("<I", 3) + b"ab\0")
        unterminated = chunk(b"DATA", struct.pack("<IBI", fnv1a("Mesh"), 1, 4) + struct.pack("<I", 3) + b"abc")
        stray = chunk(b"DATA", struct.pack("<IBI", fnv1a("Mesh"), 1, 4) + struct.pack("<I", 6) + b"ab\0cd\0")
        truncated = chunk(b"DATA", struct.pack("<IB", fnv1a("ZOrder"), 2) + struct.pack("<f", 1.0))
        for body in (bad_offset, unterminated, stray, truncated,
                     chunk(b"WAT_", b""), good + b"\x01\x02"):
            with self.assertRaises(ConfigError, msg=body[:16]):
                zero_hud2.config_chunks(config(b"hud_", name_chunk("hudtransforms"), body), b"hud_")


def fake_install(tmp):
    lvl_pc = Path(tmp) / "GameData" / "data" / "_lvl_pc"
    (lvl_pc / "load").mkdir(parents=True)
    display = scope(
        data("TipsTime", 2.75), data("TipsPrefix", "hint.tile"),
        data("RandomBackdrop", "load/spare"), data("ProgressBarTotalTime", 5.25),
        data("LoadingTextBlinkRate", 0.75), data("TipsBoxTexture", "panel", 3, 5),
        data("ProgressBarNumLEDs", 7), data("ProgressBarLEDWidth", 13),
        data("ProgressBarLEDHeight", 9), data("ProgressBarLEDSpacing", 2),
        data("ProgressBarLEDMinIntensity", 0.125), data("ProgressBarLEDTexture", "dot"))
    raw = config(b"load", name_chunk("load"),
                 data("map", "workshop"),
                 data("map", "kiln") + scope(data("TipsPrefix", "alternate"),
                                              data("RandomBackdrop", "load/paper")),
                 data("LoadDisplay") + display,
                 data("map", "cistern") + scope(data("ProgressBarTotalTime", 3.125),
                                                data("RandomBackdrop", "load/shapes")))
    (lvl_pc / "load/load.lvl").write_bytes(archive(texture("panel", 32, 28), raw[8:], texture("dot")))
    (lvl_pc / "core.lvl").write_bytes(archive(small_font(), locl({
        "hint.tile.1": "Fold a square.",
        "hint.tile.2": "Mark the opposite corner.",
        "hint.tile.4": "Unused annotation.",
        "mapname.name.cistern": "Cistern",
        "alternate.1": "Count the creases.",
    })[8:], locl({"alternate.1": " ".join(["WWW"] * 80) + " end"}, "spanish")[8:]))
    (lvl_pc / "load/shapes.lvl").write_bytes(archive(
        texture("tile_18"), chunk(b"lvl_", b"shapebox" + texture("tile_3"))))
    (lvl_pc / "load/paper.lvl").write_bytes(archive(texture("crease")))
    (lvl_pc / "load/spare.lvl").write_bytes(archive(texture("grid")))
    return lvl_pc


class LoadingScreenTests(unittest.TestCase):
    def test_world_can_inherit_both_art_and_duration(self):
        with tempfile.TemporaryDirectory() as tmp:
            screen = zero_load2.loading_screen(fake_install(tmp))
        world = screen["maps"]["workshop"]
        self.assertEqual(world["pictures"], [screen["display"]["fallbackPicture"]])
        self.assertEqual(world["durationTicks"], 315)

    def test_display_worlds_tips_and_picture_order(self):
        with tempfile.TemporaryDirectory() as tmp:
            screen = zero_load2.loading_screen(fake_install(tmp))
        display = screen["display"]
        self.assertEqual(display["tipsBox"]["border"], [3, 5])
        self.assertTrue(display["tipsBox"]["texture"].startswith("load/load/"))
        self.assertEqual([tip["key"] for tip in screen["languages"]["english"]["tips"][display["tipsPrefix"]]],
                         ["hint.tile.1", "hint.tile.2"])
        self.assertEqual(screen["languages"]["english"]["tips"][display["tipsPrefix"]][0]["text"], "Fold a square.")
        self.assertEqual(display["progressBar"]["numLeds"], 7)
        cistern = screen["maps"]["cistern"]
        # The title the screen shows is the prepared display name, not
        # the world code; a world without one gets None.
        self.assertEqual(screen["languages"]["english"]["titles"]["cistern"], "Cistern")
        self.assertEqual(cistern["nameKey"], "mapname.name.cistern")
        self.assertNotIn("kiln", screen["languages"]["english"]["titles"])
        self.assertEqual([Path(p).name for p in cistern["pictures"]], ["tile_3.png", "tile_18.png"])
        self.assertEqual(cistern["durationTicks"], 188)
        # A world without its own total time takes the display's.
        self.assertEqual(screen["maps"]["kiln"]["durationTicks"], 315)
        self.assertEqual([Path(p).name for p in screen["maps"]["kiln"]["pictures"]], ["crease.png"])

    def test_prefixes_and_complete_tip_layout_across_languages(self):
        with tempfile.TemporaryDirectory() as tmp:
            screen = zero_load2.loading_screen(fake_install(tmp))
        prefix = screen["maps"]["kiln"]["tipsPrefix"]
        english = screen["languages"]["english"]["tips"]
        self.assertEqual(english[prefix][0]["text"], "Count the creases.")
        spanish = screen["languages"]["spanish"]["tips"][prefix][0]
        encoded = spanish["text"].encode()
        start = 0
        lines = []
        for end in spanish["lineEnds"]:
            line = encoded[start:end].decode().strip()
            self.assertLessEqual(sum(30 if c == "W" else 5 for c in line), zero_load2.TIP_WIDTH)
            lines.append(line)
            start = end
        self.assertEqual(start, len(encoded))
        self.assertEqual(" ".join(lines), spanish["text"])
        self.assertGreater(len(lines), 5)
        self.assertEqual(len(lines), screen["display"]["maxTipLines"])

    def test_atomic_archive_qualified_pictures_and_retry(self):
        import zero_shell
        with tempfile.TemporaryDirectory() as tmp:
            root = fake_install(tmp)
            out = Path(tmp) / "out"
            folder = out / "common/shell"
            name = "shared"
            for stem, width in [("shapes", 4), ("paper", 8)]:
                (root / f"load/{stem}.lvl").write_bytes(archive(texture(name, width)))

            def interrupted(path, *args):
                path.write_bytes(b"partial")
                raise OSError("interrupted picture")

            with patch.object(zero_shell, "write_picture", interrupted), self.assertRaises(OSError):
                zero_load2.write_loading_screen(root, out)
            self.assertFalse((folder / "load2.json").exists())
            screen, written = zero_load2.write_loading_screen(root, out)
            paths = [folder / screen["maps"][key]["pictures"][0] for key in ("cistern", "kiln")]
            self.assertNotEqual(*paths)
            self.assertEqual([struct.unpack_from(">I", p.read_bytes(), 16)[0] for p in paths], [4, 8])
            old = paths[0].read_bytes()
            paths[0].write_bytes(b"partial")
            self.assertEqual(zero_load2.write_loading_screen(root, out)[1], written)
            self.assertEqual(paths[0].read_bytes(), old)
            (root / "load/shapes.lvl").write_bytes(archive(texture(name, 12)))
            before = (folder / "load2.json").read_bytes()
            with patch.object(zero_shell, "write_picture", interrupted), self.assertRaises(OSError):
                zero_load2.write_loading_screen(root, out)
            self.assertEqual((folder / "load2.json").read_bytes(), before)
            self.assertEqual(paths[0].read_bytes(), old)
            screen, _ = zero_load2.write_loading_screen(root, out)
            new = folder / screen["maps"]["cistern"]["pictures"][0]
            self.assertNotEqual(new, paths[0])
            self.assertEqual(struct.unpack_from(">I", new.read_bytes(), 16)[0], 12)

    def test_output_links_and_sanitized_collisions(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = fake_install(tmp)
            out = Path(tmp) / "out"
            out.mkdir()
            (out / "common").symlink_to(root, target_is_directory=True)
            (root / "ingame.lvl").write_bytes(config(b"hud_", name_chunk("1playerhud")))
            for write in [zero_load2.write_loading_screen, zero_hud2.write_hud_files]:
                with self.assertRaises(ConfigError):
                    write(root, out)
            self.assertFalse((root / "hud").exists())
            self.assertFalse((root / "shell").exists())
            (out / "common").unlink()
            (root / "load/shapes.lvl").write_bytes(archive(texture("p/a"), texture("p?a")))
            with self.assertRaises(ConfigError):
                zero_load2.write_loading_screen(root, out)

    def test_tip_border_sizes_are_checked_and_change_the_wrap_width(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = fake_install(tmp)
            load = root / "load/load.lvl"
            source = load.read_bytes()
            border = data("TipsBoxTexture", "panel", 3, 5)
            for x, y in [(-1, 5), (3, 65)]:
                load.write_bytes(source.replace(border, data("TipsBoxTexture", "panel", x, y)))
                with self.subTest(x=x, y=y), self.assertRaises(ConfigError):
                    zero_load2.loading_screen(root)
            load.write_bytes(source.replace(border, data("TipsBoxTexture", "panel", 35, 5)))
            tip = zero_load2.loading_screen(root)["languages"]["spanish"]["tips"]["alternate"][0]
            offsets = [0, *tip["lineEnds"]]
            for start, end in zip(offsets, offsets[1:]):
                line = tip["text"].encode()[start:end].decode().strip()
                self.assertLessEqual(sum(30 if c == "W" else 5 for c in line), 400 - 2 * 41)

    def test_light_height_is_bounded_by_the_footer(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = fake_install(tmp)
            config_path = root / "load/load.lvl"
            config_path.write_bytes(config_path.read_bytes().replace(
                data("ProgressBarLEDHeight", 9), data("ProgressBarLEDHeight", 143)))
            with self.assertRaises(ConfigError):
                zero_load2.loading_screen(root)

    def test_ingestion_bounds_before_pixel_decode(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = fake_install(tmp)
            # Only the dimensions change. No giant test image is ever allocated.
            raw = texture("map_art").replace(struct.pack("<IHH", 0x31545844, 4, 4),
                                             struct.pack("<IHH", 0x31545844, 65535, 65535))
            (root / "load/shapes.lvl").write_bytes(archive(raw))
            with self.assertRaises(ConfigError):
                zero_load2.loading_screen(root)
            (root / "load/shapes.lvl").write_bytes(archive(texture("map_art")))
            load = root / "load/load.lvl"
            source = load.read_bytes()
            for value in [0, 65, 1.5, "many", ("bits", 0x7fc00000)]:
                # Rebuild the enclosing archive to preserve all size fields.
                good = data("ProgressBarNumLEDs", 7)
                bad = data("ProgressBarNumLEDs", value)
                if len(good) == len(bad):
                    load.write_bytes(source.replace(good, bad))
                    with self.subTest(value=value), self.assertRaises(ConfigError):
                        zero_load2.loading_screen(root)

    def test_mixed_case_archive_names_through_both_commands(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = fake_install(tmp)
            (root / "InGame.LVL").write_bytes(config(b"hud_", name_chunk("hudtransforms")))
            (root / "core.lvl").rename(root / "Core.LvL")
            folder = root / "lOAd"
            (root / "load").rename(folder)
            for archive_path in folder.iterdir():
                archive_path.rename(folder / archive_path.name.upper())
            out = Path(tmp) / "out"
            for command in [zero_load2.main, zero_hud2.main]:
                with self.subTest(command=command.__module__):
                    self.assertEqual(command(["--game", tmp, "--out", str(out)]), 0)
            book = json.loads((out / "common/shell/load2.json").read_text())
            for world in book["maps"].values():
                self.assertTrue(world["pictures"])
                self.assertTrue(all((out / "common/shell" / name).exists() for name in world["pictures"]))

    def test_requested_base_english_and_single_language_installs(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = fake_install(tmp)
            core = root / "core.lvl"
            for available, requested, expected in [
                    (["german"], "english", "german"),
                    (["german", "english"], "DE-AT", "german"),
                    (["de", "german"], "German", "german"),
                    (["german", "de"], "de-AT", "de"),
                    (["english", "uk_english"], "UK_ENGLISH", "uk_english"),
                    (["spanish", "english"], "uk_english", "english"),
                    (["italian", "spanish"], "unavailable", "italian")]:
                with self.subTest(available=available, language=requested):
                    core.write_bytes(archive(small_font(), *(locl({"hint.tile.1": name}, name)[8:]
                                                             for name in available)))
                    screen, _ = zero_load2.write_loading_screen(root, Path(tmp) / "out", requested)
                    self.assertEqual(screen["language"], expected)
                    self.assertEqual(set(screen["languages"]), set(available))

    def test_paragraph_breaks_and_soft_wraps_cover_utf8(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = fake_install(tmp)
            source = "  Fold:\r\n\r\n  Match   the é marks.  "
            (root / "core.lvl").write_bytes(archive(small_font(), locl({"hint.tile.1": source})[8:]))
            screen = zero_load2.loading_screen(root)
        tip = screen["languages"]["english"]["tips"]["hint.tile"][0]
        self.assertEqual(tip["text"], "Fold:\n\nMatch the é marks.")
        encoded = tip["text"].encode()
        starts = [0, *tip["lineEnds"][:-1]]
        self.assertEqual([encoded[a:b].decode().strip() for a, b in zip(starts, tip["lineEnds"])],
                         ["Fold:", "", "Match the é marks."])
        ends = zero_load2.tip_line_ends("éééé\n\nmmm", {ord("é"): 7, ord("m"): 3}, 14)
        self.assertEqual(ends, [4, 9, 10, 13])

    def test_tip_keys_stop_at_a_gap(self):
        table = {fnv1a("p.1"): "a", fnv1a("p.2"): "b", fnv1a("p.4"): "d"}
        self.assertEqual(zero_load2.tip_keys("p", table), ["p.1", "p.2"])
        self.assertEqual(zero_load2.tip_keys("missing", table), [])


@unittest.skipUnless(os.environ.get("BFE_SWBF2_DIR"), "needs a 2005 install in BFE_SWBF2_DIR")
class RetailAuditTests(unittest.TestCase):
    def test_hud_explicit_properties_and_transform_bindings(self):
        huds = zero_hud2.hud_files(zero_hud2.lvl_pc_dir())

        def scopes(entries):
            for entry in entries:
                if "children" in entry:
                    yield entry
                    yield from scopes(entry["children"])

        elements = list(scopes(huds["1playerhud"]["entries"]))
        for kind, total, prop, explicit in [("text", 52, "position", 47), ("text", 52, "TextFont", 49),
                                            ("Model3D", 16, "position", 7)]:
            matching = [e for e in elements if e["name"] == kind]
            self.assertEqual(len(matching), total)
            self.assertEqual(sum(any(c["name"] == prop for c in e["children"]) for e in matching), explicit)
        bindings = zero_hud2.summarize(huds["1playerhud"]["entries"])["bindings"]
        self.assertEqual(sum("." not in key for key in bindings), 8)
        for e in scopes(huds["hudtransforms"]["entries"]):
            children = {c["name"] for c in e["children"]}
            if e["name"].startswith("Transform"):
                self.assertTrue({"EventInput", "EventOutput"} <= children)
                self.assertNotIn("EventNameFilter", children)
            if "EventNameFilter" in children:
                self.assertEqual(e["name"], "Viewport")

    def test_the_install_parses_strictly(self):
        lvl_pc = zero_hud2.lvl_pc_dir()
        self.assertIsNotNone(lvl_pc)
        huds = zero_hud2.hud_files(lvl_pc)
        self.assertEqual(len(huds), 5)
        counts = zero_hud2.summarize(huds["1playerhud"]["entries"])
        self.assertEqual((sum(counts["elements"].values()), len(counts["elements"])), (322, 77))
        self.assertEqual((sum(counts["properties"].values()), len(counts["properties"])), (1793, 81))
        screen = zero_load2.loading_screen(lvl_pc)
        self.assertGreater(len(screen["maps"]), 20)
        for locale in screen["languages"].values():
            for world in screen["maps"].values():
                group = locale["tips"][world["tipsPrefix"]]
                self.assertTrue(group)
                for tip in group:
                    if tip["text"]:
                        self.assertEqual(tip["lineEnds"][-1], len(tip["text"].encode()))
        for world in screen["maps"].values():
            self.assertGreater(len(world["pictures"]), 0, world["backdrop"])
        json.dumps(screen)  # fully JSON-able, hashes and all


if __name__ == "__main__":
    unittest.main()
