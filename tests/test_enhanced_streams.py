"""Shared Enhanced Sound streams across independent preparation jobs."""

import json
import importlib.util
import multiprocessing
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
AUDIO = all(importlib.util.find_spec(name) is not None for name in ("numpy", "soundfile", "soxr", "scipy"))
if AUDIO:
    import enhance_audio as audio


def segment(root):
    raw = bytes(range(36)) * 16
    return ("spool", "0xa1", raw, 1, 1, 0, len(raw), 24000, "effects", False, str(root))


def worker(root, gate):
    encode = audio.write_opus
    def count(path, y):
        with (root / "encodes").open("a") as log:
            log.write("encode\n")
        time.sleep(0.1)
        encode(path, y)
    with mock.patch.object(audio, "write_opus", count):
        gate.wait(15)
        audio.segment_job(segment(root))


@unittest.skipUnless(AUDIO, "requires Enhanced Sound dependencies")
class EnhancedStreamsTests(unittest.TestCase):
    @unittest.skipUnless(shutil.which("ffmpeg"), "requires ffmpeg")
    def test_denied_battle_copy_retries_only_after_access_changes(self):
        import errno
        import zero_sound_health as health
        with tempfile.TemporaryDirectory() as directory:
            public = Path(directory) / "audio"
            out = public / "enhanced"
            stream, key, report = audio.segment_job(segment(out))
            bank = out / "reed"
            target = bank / report["files"]["1ch"]
            target.parent.mkdir(parents=True)
            target.write_bytes(b"short")
            (bank / "manifest.json").write_text(json.dumps({"streams": {stream: {key: report}}}))
            access = os.access
            with (mock.patch.object(audio, "PUBLIC", public), mock.patch.object(audio, "OUT", out),
                  mock.patch.dict(os.environ, {"XDG_CACHE_HOME": directory})):
                with (mock.patch("os.access", side_effect=lambda p, m: False if Path(p) == target.parent else access(p, m)),
                      mock.patch.object(audio, "place_stream", side_effect=PermissionError(errno.EACCES, "denied")) as write):
                    audio.repair_streams()
                    saved = health.load(public.parent)
                    self.assertNotIn("retryEnhanced", saved)
                    before = health.inputs(public, enhanced=True)
                    audio.repair_streams()
                    write.assert_called_once()
                self.assertNotEqual(before, health.inputs(public, enhanced=True))
                self.assertEqual(audio.repair_streams(), (0, 1))

    def test_changed_sample_alias_is_restored_without_rechecking_other_samples(self):
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory) / "audio/enhanced"
            battle = out / "loom"
            (battle / "samples").mkdir(parents=True)
            (out / "samples").mkdir()
            samples = {key: {"file": f"samples/{key}.flac"} for key in ("reed", "yarn")}
            path = battle / "manifest.json"
            path.write_text(json.dumps({"samples": samples, "streams": {}}))
            for key, row in samples.items():
                (out / row["file"]).write_bytes(key.encode())
                (battle / row["file"]).write_bytes(key.encode())
            alias = battle / samples["reed"]["file"]
            alias.write_bytes(b"cut")
            with (mock.patch.object(audio, "OUT", out), mock.patch.object(audio, "is_swbf2", return_value=False),
                  mock.patch.object(audio, "chosen_sample", side_effect=lambda sid, bank, **kw: (sid, sid)),
                  mock.patch.object(audio, "sample_job", side_effect=lambda job: (job[0], samples[job[0]])) as verify):
                audio.repair_samples({sid: {"loom": sid} for sid in samples}, changed={"enhanced/loom/samples/reed.flac"})
                self.assertEqual(alias.read_bytes(), b"reed")
                verify.assert_called_once_with(("reed", "reed"))

    def test_next_preparation_removes_only_encoder_temporaries(self):
        import prepare
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            enhanced = root / "audio/enhanced"
            cache = enhanced / "streams/loom"
            cache.mkdir(parents=True)
            outside = root / "audio/streams"
            outside.mkdir()
            uuid = "ac" * 16
            removed = [cache / f".{uuid}.opus", enhanced / f".{uuid}.part"]
            kept = [cache / "published.opus", cache / ".settings.part", outside / f".{uuid}.opus"]
            for path in removed + kept:
                path.write_bytes(b"sample")
            (root / prepare.zero_family.profile("swbf1").side_dir).mkdir()
            with (mock.patch.object(prepare, "PUBLIC", root), mock.patch.object(prepare, "GAME", root),
                  mock.patch.object(prepare, "prepare_all", return_value=0),
                  mock.patch.dict(os.environ, {"BFE_FAMILY": "swbf1"}),
                  mock.patch.object(sys, "argv", ["prepare.py", "--no-enhanced-sound"])):
                self.assertEqual(prepare.main(), 0)
            self.assertFalse(any(path.exists() for path in removed))
            self.assertTrue(all(path.read_bytes() == b"sample" for path in kept))

    @unittest.skipUnless(shutil.which("ffmpeg"), "requires ffmpeg")
    def test_2005_interleaved_loop_uses_declared_frames_and_keeps_channel_order(self):
        with tempfile.TemporaryDirectory() as directory, mock.patch.dict(os.environ, {"BFE_FAMILY": "swbf2"}):
            root = Path(directory) / "audio/enhanced"
            # Two stereo substreams, two blocks per interleave. The final block is playable.
            def stereo_block(left, right):
                return struct.pack("<hBBhBB", left, 0, 0, right, 0, 0) + bytes(64)
            raw = stereo_block(100, 200) * 2 + stereo_block(300, 400) * 2
            index = {"format": 5, "channels": 2, "substreams": 2, "substreamInterleave": 144,
                     "segments": {"bed": [0, 216, 240, 24000]}}
            args = audio.segment_args("loom", "bed", index, raw, "ambience", True, root)
            with mock.patch.object(audio, "enhance", wraps=audio.enhance) as enhance:
                _, _, report = audio.segment_job(args)
            self.assertEqual((enhance.call_args.args[0][:, 0] * 32768).tolist(), [100, 200, 300, 400])
            self.assertEqual(report["channels"], 4)
            self.assertEqual(report["source_loop"], [0, 120])
            self.assertEqual(report["loop_frames"], [0, 240])
            self.assertTrue(audio.complete_opus(root / report["files"]["4ch"], 240))

    def test_2005_empty_appended_voice_slot_keeps_its_identity(self):
        with tempfile.TemporaryDirectory() as directory, mock.patch.dict(os.environ, {"BFE_FAMILY": "swbf2"}):
            root = Path(directory) / "audio/enhanced"
            index = {"format": 5, "channels": 1, "segments": {"slot": [0, 0, 0, 24000]}}
            args = audio.segment_args("spool", "slot", index, b"", "voice", False, root)
            _, key, report = audio.segment_job(args)
            self.assertEqual((key, report["frames"], report["files"], report["fallback"]), ("slot", 0, {}, True))
            self.assertEqual(report["reason"], "empty")

    @unittest.skipUnless(shutil.which("ffmpeg"), "requires ffmpeg")
    def test_2005_short_music_keeps_every_layout_at_the_source_loop(self):
        with tempfile.TemporaryDirectory() as directory, mock.patch.dict(os.environ, {"BFE_FAMILY": "swbf2"}):
            root = Path(directory) / "audio/enhanced"
            index = {"format": 5, "channels": 2, "segments": {"note": [0, 72, 32, 32000]}}
            args = audio.segment_args("chime", "note", index, bytes(72), "music", True, root)
            _, _, report = audio.segment_job(args)
            self.assertEqual(report["source_loop"], [0, 32])
            self.assertEqual(report["loop_frames"], [0, 48])
            self.assertEqual(set(report["files"]), {"stereo", "5.1", "7.1"})
            for name in report["files"].values():
                self.assertTrue(audio.complete_opus(root / name, 48))

    def test_conflicting_sublevel_settings_share_one_treatment_per_preparation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with mock.patch.object(audio, "OUT", root / "audio/enhanced"):
                for i, first in enumerate(["music", "ambience"]):
                    session = root / str(i)
                    session.mkdir()
                    self.assertEqual(audio.shared_stream_settings(session, "thread", (first, False)), (first, False))
                    self.assertEqual(audio.shared_stream_settings(session, "thread", ("voice", True)), (first, False))
                    self.assertEqual(audio.shared_stream_settings(session, "needle", ("voice", True)), ("voice", True))

    @unittest.skipUnless(shutil.which("ffmpeg"), "requires ffmpeg")
    def test_parallel_sublevels_encode_a_contended_segment_once(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "audio/enhanced"
            root.mkdir(parents=True)
            ctx = multiprocessing.get_context("spawn")
            gate = ctx.Event()
            jobs = [ctx.Process(target=worker, args=(root, gate)) for _ in range(2)]
            for job in jobs:
                job.start()
            gate.set()
            for job in jobs:
                job.join(30)
                if job.is_alive():
                    job.kill()
                    job.join()
                self.assertEqual(job.exitcode, 0)
            self.assertEqual((root / "encodes").read_text().splitlines(), ["encode"])

    def test_interrupted_encoder_preserves_published_stream(self):
        def fail(cmd, **kwargs):
            Path(cmd[-1]).write_bytes(b"unfinished")
            raise subprocess.CalledProcessError(1, cmd)
        with tempfile.TemporaryDirectory() as directory, mock.patch("subprocess.run", fail):
            dest = Path(directory) / "pulse.opus"
            for previous in (None, b"previous audio"):
                if previous is not None:
                    dest.write_bytes(previous)
                with self.assertRaises(subprocess.CalledProcessError):
                    audio.write_opus(dest, audio.np.zeros((1, 1024)))
                self.assertEqual(dest.read_bytes() if dest.exists() else None, previous)
                self.assertEqual(list(dest.parent.iterdir()), [] if previous is None else [dest])

    @unittest.skipUnless(shutil.which("ffmpeg"), "requires ffmpeg")
    def test_repair_keeps_a_decodable_stream_and_remembers_failed_attempts(self):
        with tempfile.TemporaryDirectory() as directory:
            public = Path(directory) / "audio"
            out = public / "enhanced"
            stream, key, report = audio.segment_job(segment(out))
            master = out / report["files"]["1ch"]
            battle = out / "linen"
            alias = battle / report["files"]["1ch"]
            alias.parent.mkdir(parents=True)
            alias.hardlink_to(master)
            report["frames"] += 1
            (battle / "manifest.json").write_text(json.dumps({"streams": {stream: {key: report}}}))
            broken = out / "silk/manifest.json"
            broken.parent.mkdir()
            broken.write_text('{"streams":')
            with (mock.patch.object(audio, "OUT", out), mock.patch.object(audio, "PUBLIC", public),
                  mock.patch.dict(os.environ, {"XDG_CACHE_HOME": directory}),
                  mock.patch.object(audio, "encode_segment", side_effect=AssertionError("healthy encode"))):
                banks = []
                self.assertEqual(audio.repair_streams(banks.append), (0, 0))
                self.assertEqual(banks, ["linen"])
                with mock.patch.object(audio, "complete_opus", side_effect=AssertionError("reread healthy audio")):
                    self.assertEqual(audio.repair_streams(), (0, 0))
                master.write_bytes(b"incomplete")
                self.assertEqual(audio.repair_streams(), (0, 0))  # Missing source bank.
                with mock.patch.object(audio, "segment_lock", side_effect=AssertionError("repeat repair")):
                    self.assertEqual(audio.repair_streams(), (0, 0))

    @unittest.skipUnless(shutil.which("ffmpeg"), "requires ffmpeg")
    def test_temporary_encode_failure_can_retry_without_a_source_change(self):
        import errno
        import zero_sound_health as health
        with tempfile.TemporaryDirectory() as directory:
            public = Path(directory) / "audio"
            out = public / "enhanced"
            args = segment(out)
            stream, key, report = audio.segment_job(args)
            indexes = public / "streams"
            indexes.mkdir(parents=True)
            (indexes / "spool.bin").write_bytes(args[2])
            (indexes / "spool.json").write_text(json.dumps({"channels": 1, "segments": {key: [0, args[6], 1024, args[7]]}}))
            file = report["files"]["1ch"]
            master = out / file
            master.write_bytes(b"short")
            with (mock.patch.object(audio, "OUT", out), mock.patch.object(audio, "PUBLIC", public),
                  mock.patch.dict(os.environ, {"XDG_CACHE_HOME": str(public.parent / "cache")})):
                with mock.patch.object(audio, "encode_segment", side_effect=OSError(errno.ENOSPC, "disk full")) as encode:
                    self.assertEqual(audio.repair_streams(), (0, 0))
                    encode.assert_called_once()
                saved = health.load(public.parent)
                self.assertEqual(saved["streams"][file][4], "retry")
                self.assertIn("disk full", saved["streams"][file][5])
                self.assertTrue(health.retry_due(saved, public, preparing=True))
                with mock.patch("shutil.disk_usage", return_value=type("Space", (), {"free": saved["retryEnhanced"]["free"] + (2 << 20)})()):
                    self.assertTrue(health.retry_due(saved, public))
                self.assertEqual(audio.repair_streams(), (1, 0))
                self.assertNotIn("retryEnhanced", health.load(public.parent))

    @unittest.skipUnless(shutil.which("ffmpeg"), "requires ffmpeg")
    def test_truncated_shared_stream_and_copied_alias_repair_automatically(self):
        with tempfile.TemporaryDirectory() as directory:
            public = Path(directory) / "audio"
            out = public / "enhanced"
            args = segment(out)
            stream, key, report = audio.segment_job(args)
            index = public / "streams"
            index.mkdir(parents=True)
            (index / "spool.bin").write_bytes(args[2])
            (index / "spool.json").write_text(json.dumps({"channels":1, "segments":{key:[0,args[6],1024,args[7]]}}))
            file = report["files"]["1ch"]
            master = out / file
            def decode(p):
                return subprocess.check_output(["ffmpeg", "-v", "error", "-i", str(p), "-f", "s16le", "-"])
            expected = decode(master)
            # An old run can have a segment receipt before its battle manifest exists.
            master.write_bytes(master.read_bytes()[:100])
            with mock.patch.object(audio, "OUT", out), mock.patch.object(audio, "PUBLIC", public):
                self.assertEqual(audio.repair_streams(), (1, 0))
                self.assertEqual(audio.repair_streams(), (0, 0))
            self.assertEqual(decode(master), expected)
            for sub in ("linen", "cotton"):
                dest = out / sub / file
                dest.parent.mkdir(parents=True)
                (out / sub / "manifest.json").write_text(json.dumps({"streams": {stream: {key: report}}}))
                if sub == "linen":
                    dest.hardlink_to(master)
                else:
                    dest.write_bytes(master.read_bytes()[:-9])
            master.write_bytes(master.read_bytes()[:100])
            import prepare
            with (mock.patch.object(prepare, "PUBLIC", public.parent),
                  mock.patch.dict(os.environ, {"BFE_FAMILY":"swbf1", "BFE_CONTENT_DIR":str(public.parent),
                                               "XDG_CACHE_HOME":str(public.parent / "cache")}),
                  mock.patch.object(prepare, "check_install", return_value={"ok":True, "problems":[]}),
                  mock.patch.object(sys, "argv", ["prepare.py", "--check"])):
                self.assertEqual(prepare.main(), 0)
                self.assertEqual(master.stat().st_size, 100)
                with mock.patch.object(sys, "argv", ["prepare.py", "--maintain-sound"]):
                    self.assertEqual(prepare.main(), 0)
                    with mock.patch.object(prepare, "run_tool", side_effect=AssertionError("healthy launch ran a step")):
                        self.assertEqual(prepare.main(), 0)
            for folder in (out, out/"linen", out/"cotton"):
                self.assertEqual(decode(folder / file), expected)
            with mock.patch.object(audio, "OUT", out), mock.patch.object(audio, "PUBLIC", public):
                self.assertEqual(audio.repair_streams(), (0, 0))


if __name__ == "__main__":
    unittest.main()
