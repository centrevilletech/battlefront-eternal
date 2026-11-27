"""The original game's sound metadata can be updated without decoding audio."""
import json
import os
from pathlib import Path
import struct
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from zero_hash import fnv1a
import zero_sound


def chunk(tag, data):
    return tag + struct.pack("<I", len(data)) + data + bytes(-len(data) % 4)


def entry(name, *args, children=b""):
    values = b"".join(struct.pack("<I", fnv1a(x)) if isinstance(x, str) else struct.pack("<f", x) for x in args)
    field = chunk(b"DATA", struct.pack("<IB", fnv1a(name), len(args)) + values)
    return field + chunk(b"SCOP", children) if children else field


def curves():
    bindings = [("reefs", "yellow", "mist", 8.125), ("wind", "blue", "pitch"), ("wind", "blue", "pitch")]
    fields = entry("Name", "oar") + b"".join(entry("ParameterGraph", *values) for values in bindings)
    raw = entry("SoundParameterized", children=entry("Name", "orchard") + entry("Layer", children=fields))
    encoded = [[zero_sound.hexname(fnv1a(v)) if isinstance(v, str) else v for v in row] for row in bindings]
    return raw, encoded


class OriginalSoundTest(unittest.TestCase):
    def test_invalid_archive_retries_only_when_an_input_changes(self):
        import prepare
        import zero_sound_upgrade as upgrade
        import zero_sound_health as health
        from types import SimpleNamespace
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            game = root / "GameData/Data/_LVL_PC"
            game.mkdir(parents=True)
            archive = game / "music.lvl"
            archive.write_bytes(b"cut")
            (root / "audio").mkdir()
            (root / "audio/index.json").write_text("{}")
            def refresh(*, damaged, **kwargs):
                damaged.add("spool")
            with (patch.object(prepare, "PUBLIC", root), patch.object(upgrade, "prepared_game", return_value=game),
                  patch.object(upgrade, "refresh_prepared", refresh),
                  patch.object(prepare, "damaged_sound_maps", return_value=["loom"]),
                  patch.object(prepare, "run_tool", side_effect=[prepare.Ran(False, ["Invalid archive"]), True]) as run,
                  patch.dict(os.environ, {"XDG_CACHE_HOME": directory, "BFE_FAMILY": "swbf1"})):
                options = SimpleNamespace(no_enhanced_sound=True)
                prepare.maintain_sound(options)
                with patch("time.time", return_value=1e12):
                    prepare.maintain_sound(options)
                    self.assertEqual(run.call_count, 1)
                    self.assertEqual(health.load(root)["blockedManifests"]["loom"], "Invalid archive")
                    archive.write_bytes(b"complete source")
                    prepare.maintain_sound(options)
                    self.assertEqual(run.call_count, 2)
                    self.assertNotIn("blockedManifests", health.load(root))

    def test_preparation_cleans_only_its_interrupted_original_sound_writes(self):
        import prepare
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            audio = root / "audio"
            bank = audio / "loom"
            (bank / "tmpabcdefgh").mkdir(parents=True)
            (bank / "tmpother123").mkdir()
            (audio / "streams").mkdir()
            removed = [audio / "index.json.part", bank / ".bfe-bank-ab12cd34.part",
                       bank / (".bfe-manifest-" + "a" * 32 + ".part"), bank / "tmpabcdefgh/sound.json",
                       audio / "streams/ripple.bin.1234.part"]
            kept = [bank / "tmpother123/notes", bank / "notes.part", audio / "streams/ripple.bin.part"]
            for path in removed + kept:
                path.write_bytes(b"marker")
            (root / prepare.zero_family.profile("swbf1").side_dir).mkdir()
            with (patch.object(prepare, "PUBLIC", root), patch.object(prepare, "GAME", root),
                  patch.object(prepare, "prepare_all", return_value=0),
                  patch.dict(os.environ, {"BFE_FAMILY": "swbf1"}), patch.object(sys, "argv", ["prepare.py"])):
                self.assertEqual(prepare.main(), 0)
            self.assertFalse(any(path.exists() for path in [*removed, bank / "tmpabcdefgh"]))
            self.assertTrue(all(path.read_bytes() == b"marker" for path in kept))

    def test_shared_bank_keeps_its_battle_setup_across_a_mission_interruption(self):
        import extract
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "sound.json"
            path.write_text('{"setup":"previous"}')
            cut = True
            def battle(name, *, mission, manifest_writer=zero_sound.write_manifest):
                if cut and mission == "tide":
                    raise KeyboardInterrupt()
                manifest_writer(path, {"setup": mission})
            missions = {"tide": {"historical": False}, "crest": {"historical": True}}
            with patch.object(extract, "map_missions", return_value=missions), patch.object(extract, "extract_audio", battle):
                with self.assertRaises(KeyboardInterrupt):
                    extract.extract_map_audio("bowl")
                self.assertEqual(json.loads(path.read_bytes()), {"setup": "previous"})
                cut = False
                extract.extract_map_audio("bowl")
                self.assertEqual(json.loads(path.read_bytes()), {"setup": "tide"})

    @unittest.skipUnless(os.name == "posix", "POSIX create permissions")
    def test_atomic_sound_outputs_use_the_same_mode_as_plain_creation(self):
        import stat
        import zero_sound_health as health
        for mask in (0o022, 0o077):
            with self.subTest(mask=mask), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                previous = os.umask(mask)
                try:
                    reference = root / "ordinary"
                    reference.write_bytes(b"plain create")
                    zero_sound.write_bank(root / "bank.bin", b"invented samples")
                    zero_sound.write_manifest(root / "sound.json", {"sample": 9})
                    with patch.dict(os.environ, {"XDG_CACHE_HOME": directory}):
                        health.save(root, {"version": health.VERSION})
                        verdict = health.record_path(root)
                    for path in (root / "bank.bin", root / "sound.json", verdict):
                        self.assertEqual(stat.S_IMODE(path.stat().st_mode), stat.S_IMODE(reference.stat().st_mode))
                finally:
                    os.umask(previous)

    def test_a_temporary_original_bank_failure_retries_when_space_returns(self):
        import errno
        import prepare
        import zero_sound_health as health
        import zero_sound_upgrade as upgrade
        from types import SimpleNamespace
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "audio").mkdir()
            (root / "audio/index.json").write_bytes(b"{}")
            with (patch.object(prepare, "PUBLIC", root), patch.object(upgrade, "PUBLIC", root),
                  patch.object(upgrade, "refresh_prepared", side_effect=[OSError(errno.ENOSPC, "disk full"), 0]) as refresh,
                  patch.dict(os.environ, {"XDG_CACHE_HOME": folder, "BFE_FAMILY": "swbf1"})):
                options = SimpleNamespace(no_enhanced_sound=True)
                prepare.maintain_sound(options)
                prepare.maintain_sound(options)
                self.assertEqual(refresh.call_count, 1)
                token = health.load(root)["retryManifests"]
                self.assertIn("disk full", token["reason"])
                with patch("shutil.disk_usage", return_value=SimpleNamespace(free=token["free"] + (2 << 20))):
                    prepare.maintain_sound(options)
                self.assertEqual(refresh.call_count, 2)
                self.assertNotIn("retryManifests", health.load(root))

    def test_cache_that_cannot_save_verdicts_skips_expensive_checks(self):
        import prepare
        from types import SimpleNamespace
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "audio/enhanced").mkdir(parents=True)
            cache = root / "cache-file"
            cache.touch()
            with (patch.object(prepare, "PUBLIC", root), patch.object(prepare, "run_tool") as encoder,
                  patch.dict(os.environ, {"XDG_CACHE_HOME": str(cache)}), patch("builtins.print") as log):
                for _ in range(2):
                    prepare.maintain_sound(SimpleNamespace(no_enhanced_sound=False))
                encoder.assert_not_called()
                self.assertIn("cannot save", str(log.call_args_list))
                with patch.object(prepare, "has_modules", return_value=True), patch.object(prepare, "ffmpeg_ok", return_value=True):
                    prepare.maintain_sound(SimpleNamespace(no_enhanced_sound=False), locked=True)
                encoder.assert_called_once_with("enhance_audio.py", "--repair-streams")

    def test_unwritable_cache_still_allows_index_repair_at_launch(self):
        import prepare
        import zero_sound_upgrade as upgrade
        from types import SimpleNamespace
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            audio = root / "audio"
            audio.mkdir()
            live = audio / "index.json"
            live.write_text('{"maps":')
            (root / "core.lvl").touch()
            (root / ".prepare.json").write_text('{"done":["map:loom"]}')
            cache = root / "cache-file"
            cache.touch()
            expected = {"maps": {"loom": "reed"}, "missions": {"weave": "reed"}}
            with (patch.object(prepare, "PUBLIC", root), patch.object(upgrade, "PUBLIC", root),
                  patch.object(upgrade, "GAME", root), patch("zero_sound_index.expected_index", return_value=expected),
                  patch.object(upgrade, "refresh") as refresh, patch.object(prepare, "run_tool") as tool,
                  patch.dict(os.environ, {"XDG_CACHE_HOME": str(cache), "BFE_FAMILY": "swbf1"})):
                prepare.maintain_sound(SimpleNamespace(no_enhanced_sound=False))
            self.assertEqual(json.loads(live.read_bytes()), expected)
            refresh.assert_not_called()
            tool.assert_not_called()

    def test_partial_bank_write_leaves_the_previous_bank_readable(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            dst = root / "bank.bin"
            dst.write_bytes(b"old samples")
            def fail(staged, target):
                self.assertEqual(target.read_bytes(), b"old samples")
                self.assertEqual(staged.read_bytes(), b"complete samples")
                raise OSError("interrupted")
            with patch.object(Path, "replace", fail), self.assertRaises(OSError):
                zero_sound.write_bank(dst, b"complete samples")
            self.assertEqual(list(root.iterdir()), [dst])
            self.assertEqual(dst.read_bytes(), b"old samples")
            zero_sound.write_bank(dst, b"complete samples")
            self.assertEqual(dst.read_bytes(), b"complete samples")

    def test_partial_bank_is_recognized_from_the_last_padded_sample_span(self):
        import zero_sound_upgrade as upgrade
        with tempfile.TemporaryDirectory() as directory:
            audio = Path(directory)
            bank = audio / "quilt"
            bank.mkdir()
            (bank / "sound.json").write_text(json.dumps({"map": "loom", "sublevel": "quilt", "objects": [],
                 "samples": {"warp": [4, 3, 8000]}, "mission": {"lowReinforcementsVo": []}}))
            for size in (4, 10, 12):
                (bank / "bank.bin").write_bytes(bytes(size))
                damaged = set()
                upgrade.refresh(audio, audio, damaged)
                self.assertEqual(damaged, set() if size == 12 else {"quilt"})

    def test_maintenance_rebuilds_a_cut_index_from_the_recorded_install(self):
        import prepare
        import zero_sound_upgrade as upgrade
        from types import SimpleNamespace
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "prepared"
            audio = root / "audio"
            audio.mkdir(parents=True)
            (audio / "index.json").write_bytes(b'{"miss')
            install = Path(directory) / "player-copy"
            game = install / "GameData" / "Data" / "_LVL_PC"
            game.mkdir(parents=True)
            (game / "core.lvl").touch()
            (root / ".prepare.json").write_text(json.dumps({"game": str(game), "done": ["map:loom"]}))
            expected = {"missions": {"loomg": "quilt"}, "maps": {"loom": "quilt"}}
            (audio / "quilt").mkdir()
            (audio / "quilt/sound.json").write_text(json.dumps({"map": "loom", "sublevel": "quilt", "objects": [],
                                                                "mission": {"lowReinforcementsVo": []}}))
            absent = Path(directory) / "gone"
            with (patch.object(prepare, "PUBLIC", root), patch.object(upgrade, "PUBLIC", root),
                  patch.object(prepare, "GAME", absent), patch.object(upgrade, "GAME", absent),
                  patch("zero_sound_index.expected_index", return_value=expected) as rebuild,
                  patch.object(prepare, "run_tool") as run,
                  patch.dict(os.environ, {"BFE_FAMILY": "swbf1", "XDG_CACHE_HOME": directory})):
                options = SimpleNamespace(no_enhanced_sound=True, jobs=1)
                prepare.maintain_sound(options)
                self.assertEqual(json.loads((audio / "index.json").read_bytes()), expected)
                rebuild.assert_called_once_with([("loom", root / "maps/loom/map.json")], game)
                prepare.maintain_sound(options)
                self.assertEqual(rebuild.call_count, 1)
                run.assert_not_called()

    def test_restoring_an_install_triggers_deferred_manifest_maintenance_once(self):
        import prepare
        import zero_sound_upgrade as upgrade
        from types import SimpleNamespace
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "prepared"
            game = Path(directory) / "game"
            game.mkdir()
            (root / "audio").mkdir(parents=True)
            (root / "audio/index.json").write_text('{"missions":{}}')
            with (patch.object(prepare, "PUBLIC", root), patch.object(upgrade, "GAME", game),
                  patch.object(upgrade, "PUBLIC", root), patch.object(upgrade, "refresh_prepared") as refresh,
                  patch.dict(os.environ, {"XDG_CACHE_HOME": directory, "BFE_FAMILY": "swbf1"})):
                opts = SimpleNamespace(no_enhanced_sound=True)
                prepare.maintain_sound(opts)
                prepare.maintain_sound(opts)
                self.assertEqual(refresh.call_count, 1)
                (game / "core.lvl").touch()
                prepare.maintain_sound(opts)
                prepare.maintain_sound(opts)
                self.assertEqual(refresh.call_count, 2)

    def test_replacing_a_sound_manifest_keeps_the_previous_file_complete(self):
        import zero_shell
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "sound.json"
            path.write_bytes(b'{"previous":true}')
            previous = root / "previous.json"
            previous.hardlink_to(path)
            zero_shell.write_sound_sublevel(root, [], {"banks": []}, {}, b"")
            self.assertEqual(previous.read_bytes(), b'{"previous":true}')
            self.assertEqual(json.loads(path.read_bytes())["sublevel"], "shell")

    def test_install_check_stays_read_only_when_optional_sound_repair_fails(self):
        import prepare
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "content"
            bank = root / "audio/enhanced/canvas/manifest.json"
            bank.parent.mkdir(parents=True)
            bank.write_bytes(b'{"streams":')
            with (patch.object(prepare, "PUBLIC", root), patch.dict(os.environ, {"XDG_CACHE_HOME": directory}),
                  patch.object(prepare, "has_modules", return_value=True), patch.object(prepare, "ffmpeg_ok", return_value=True),
                  patch.object(prepare, "run_tool", side_effect=OSError("encoder unavailable")) as encoder,
                  patch.object(prepare, "check_install", return_value={"ok": True, "problems": []}),
                  patch.object(prepare, "emit") as progress):
                with patch.object(sys, "argv", ["prepare.py", "--check"]):
                    self.assertEqual(prepare.main(), 0)
                    encoder.assert_not_called()
                    progress.assert_called_once_with("check", ok=True, problems=[])
                with patch.object(sys, "argv", ["prepare.py", "--maintain-sound"]):
                    self.assertEqual(prepare.main(), 0)
                    self.assertEqual(prepare.main(), 0)
                    encoder.assert_called_once()
                    self.assertTrue(any(call.kwargs.get("status") == "failed" for call in progress.call_args_list))
                self.assertEqual(bank.read_bytes(), b'{"streams":')

    def test_unreadable_sound_manifests_schedule_their_map_again(self):
        import zero_sound_upgrade as upgrade
        with tempfile.TemporaryDirectory() as tmp:
            audio = Path(tmp) / "audio"
            path = audio / "quilt" / "sound.json"
            path.parent.mkdir(parents=True)
            (audio / "index.json").write_text(json.dumps({"missions": {"hill": "quilt"}}))
            for data in (b"", b'{"sublev', b'{"objects":[{"gain":1.', b"[]", b"null",
                         b'{"objects":{},"sublevel":"quilt"}', b'{"objects":[{"type":3}]}'):
                with self.subTest(data=data):
                    path.write_bytes(data)
                    damaged = set()
                    self.assertEqual(upgrade.refresh(audio, Path(tmp)), 0)
                    self.assertEqual(upgrade.refresh(audio, Path(tmp), damaged), 0)
                    self.assertEqual(damaged, {"quilt"})
                    self.assertEqual(path.read_bytes(), data)
            path.unlink()
            damaged = set()
            upgrade.refresh(audio, Path(tmp), damaged)
            self.assertEqual(damaged, {"quilt"})
            path.touch()
            with patch.object(Path, "read_bytes", side_effect=PermissionError("unreadable")):
                damaged.clear()
                self.assertEqual(upgrade.refresh(audio, Path(tmp), damaged), 0)
                self.assertIn("quilt", damaged)

    def test_voice_rules_use_watched_and_listener_teams_count_and_fraction(self):
        calls = [("SetLowReinforcementsVoiceOver", [7, 9, "lantern", 12, 0]),
                 ("SetLowReinforcementsVoiceOver", [9, 7, "cloud", .375, 1])]
        self.assertEqual(zero_sound.read_mission_audio(calls)["lowReinforcementsVo"],
                         [[9, 12, "lantern", False, 7], [7, .375, "cloud", True, 9]])
        self.assertEqual(zero_sound.read_mission_audio([])["lowReinforcementsVo"], [])

    def test_both_games_accept_a_voice_threshold_without_a_fraction_flag(self):
        from zero_sound2 import mission_audio
        calls = [("SetLowReinforcementsVoiceOver", [3, 8, "reed", 14])]
        for read in (zero_sound.read_mission_audio, mission_audio):
            self.assertEqual(read(calls)["lowReinforcementsVo"], [[8, 14, "reed", False, 3]])

    def test_two_missions_with_one_bank_can_use_different_voice_rules(self):
        records = {"hill": [[7, 4, "lantern", False, 9]], "hill_h": [], "hill2": []}
        setup = zero_sound.voice_setup(records, "hill")
        self.assertEqual(setup["lowReinforcementsVo"], records["hill"])
        self.assertEqual(setup["lowReinforcementsVoByMission"], {"hill_h": [], "hill2": []})
        self.assertNotIn("lowReinforcementsVoByMission", zero_sound.voice_setup({"hill": []}, "hill"))

    def test_voice_only_upgrade_keeps_sample_and_curve_data(self):
        import zero_sound_upgrade as upgrade
        with tempfile.TemporaryDirectory() as tmp:
            audio = Path(tmp) / "audio"
            path = audio / "hill" / "sound.json"
            path.parent.mkdir(parents=True)
            path.write_text(json.dumps({"sublevel": "hill", "map": "hill", "objects": [], "mission": {}}))
            (audio / "index.json").write_text(json.dumps({"missions": {"hill_h": "hill", "hill": "hill"}}))
            rules = {"hill_h": [], "hill": [[3, 12, "cloud", False, 5]]}
            with (patch.object(upgrade, "source_objects", side_effect=AssertionError("read sample config")),
                  patch.object(upgrade, "missions", return_value={}),
                  patch.object(zero_sound, "rules_for_sublevel", return_value=rules) as decode):
                self.assertEqual(upgrade.refresh(audio, Path(tmp)), 1)
                self.assertEqual(upgrade.refresh(audio, Path(tmp)), 0)
                decode.assert_called_once()
            self.assertEqual(json.loads(path.read_text())["mission"], zero_sound.voice_setup(rules, "hill"))

    def test_every_curve_in_order_and_released_single_binding(self):
        raw, values = curves()
        obj, = zero_sound.config_objects(raw, [("snd_", 0, len(raw))])
        layer, = obj["Layer.children"]
        self.assertEqual(layer["ParameterGraph"], values[-1])
        self.assertEqual(layer["ParameterGraphs"], values)

    def test_update_only_outdated_sound_manifest_once(self):
        import zero_sound_upgrade as upgrade
        raw, values = curves()
        objects = zero_sound.config_objects(raw, [("snd_", 0, len(raw))])
        with tempfile.TemporaryDirectory() as tmp:
            audio = Path(tmp) / "audio"
            dest = audio / "orchard" / "sound.json"
            dest.parent.mkdir(parents=True)
            bank = dest.with_name("bank.bin")
            bank.write_bytes(b"unchanged sample")
            old = json.loads(json.dumps(objects))
            old[0]["Layer.children"][0].pop("ParameterGraphs")
            old[0]["Layer.children"][0]["customMix"] = [3, 7]
            dest.write_text(json.dumps({"sublevel": "orchard", "map": "hill", "objects": old,
                                        "mission": {"banks": ["orchard"], "lowReinforcementsVo": []}}))
            (audio / "index.json").write_text(json.dumps({"missions": {"orchard_run": "orchard"}}))
            with patch.object(upgrade, "source_objects", return_value=objects) as source:
                self.assertEqual(upgrade.refresh(audio, Path(tmp)), 1)
                before = dest.stat().st_mtime_ns
                self.assertEqual(upgrade.refresh(audio, Path(tmp)), 0)
                self.assertEqual(dest.stat().st_mtime_ns, before)
                source.assert_called_once()
            layer = json.loads(dest.read_text())["objects"][0]["Layer.children"][0]
            self.assertEqual(layer["ParameterGraphs"], values)
            self.assertEqual(layer["customMix"], [3, 7])
            self.assertEqual(bank.read_bytes(), b"unchanged sample")
