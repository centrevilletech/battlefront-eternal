"""Concurrent battles publish shared enhancement metadata atomically."""
import json
import os
import struct
import subprocess
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
import enhance_audio
from enhance_audio import cached_sample, cached_segment, write_record


class AudioCacheTest(unittest.TestCase):
    def test_a_damaged_bank_does_not_hide_other_battles_sample_requests(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ("reed", "stone", "twine"):
                bank = root / name
                bank.mkdir()
                (bank / "sound.json").write_text(json.dumps({"kind": name, "samples": {"note": [2, 7, 11]}}))
            (root / "stone/sound.json").write_text("{")
            with (mock.patch.object(enhance_audio, "PUBLIC", root),
                  mock.patch.object(enhance_audio, "sample_kinds", side_effect=lambda m: ({"note": m["kind"]}, set()))):
                choices = enhance_audio.battle_sample_choices()
            self.assertEqual(set(choices["note"]), {"reed", "twine"})

    def test_each_battle_uses_its_own_treatment_in_both_jobs_and_on_repair(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            audio = root / 'audio'
            sid = '0x12345678'
            for level, label in [('yarn', 'mvt_foley'), ('spindle', 'thread_fire')]:
                bank = audio / level
                bank.mkdir(parents=True)
                (bank / 'bank.bin').write_bytes(struct.pack('<1024h', *([1000, 500, -700, -300] * 256)))
                (bank / 'sound.json').write_text(json.dumps({
                    'map': level, 'samples': {sid: [0, 1024, 24000]}, 'names': {'sound': label},
                    'objects': [{'type': 'SoundProperties', 'Name': 'sound', 'SampleList': [[sid]]}]}))
            shell = root / 'common/shell'
            shell.mkdir(parents=True)
            (shell / 'shell.json').write_text(json.dumps({'maps': [{'id': 'yarn'}, {'id': 'spindle'}]}))
            env = dict(os.environ, BFE_FAMILY='swbf1', BFE_CONTENT_DIR=str(root))
            tool = [sys.executable, enhance_audio.__file__]
            procs = [subprocess.Popen([*tool, n, '--only', 'samples', '--jobs', '1'], env=env)
                     for n in ('spindle', 'yarn')]
            self.assertEqual([p.wait() for p in procs], [0, 0])
            dest = audio / 'enhanced'
            def report(n):
                return json.loads((dest / n / 'manifest.json').read_bytes())['samples'][sid]
            self.assertEqual(report('spindle')['kind'], 'weapons')
            self.assertEqual(report('yarn')['kind'], 'foley')
            masters = sorted((dest / 'samples').glob(sid + '-*.json'))
            self.assertEqual(len(masters), 2)
            self.assertFalse((dest / 'samples' / (sid + '.flac')).exists())
            expected = {bank: report(bank) for bank in ('yarn', 'spindle')}
            for bank, item in expected.items():
                if item['file'] is not None:
                    self.assertTrue((dest / bank / item['file']).is_file())
            # Change the playback choice on disk and repair without a usable mission index.
            (audio / 'index.json').write_text('{"maps":{},"missions":{}}')
            path = dest / 'yarn/manifest.json'
            manifest = json.loads(path.read_bytes())
            manifest['samples'][sid] = expected['spindle']
            path.write_text(json.dumps(manifest))
            subprocess.run([*tool, '--repair-streams'], env=env, check=True)
            self.assertEqual({bank: report(bank) for bank in expected}, expected)
            stamps = {p: p.stat().st_mtime_ns for p in masters}
            subprocess.run([*tool, '--repair-streams'], env=env, check=True)
            self.assertTrue(all(p.stat().st_mtime_ns == t for p, t in stamps.items()))

    def test_concurrent_shared_record_writers(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / 'shared.json'
            with ThreadPoolExecutor(max_workers=16) as pool:
                list(pool.map(lambda n: write_record(target, 'weapon', False, {'writer': n}), range(128)))
            record = json.loads(target.read_text())
            self.assertIn(record['report']['writer'], range(128))
            self.assertEqual(record['kind'], 'weapon')
            self.assertFalse(record['loop'])
            self.assertEqual(list(Path(directory).glob('*.part')), [])

    def test_finished_sample_is_reused(self):
        with tempfile.TemporaryDirectory() as directory, mock.patch.object(enhance_audio, 'OUT', Path(directory)):
            samples = Path(directory) / 'samples'
            samples.mkdir()
            flac = samples / '0x1234.flac'
            flac.write_bytes(b'fLaC')
            write_record(samples / '0x1234.json', 'weapon', False, {'file': 'samples/0x1234.flac', 'rate': 48000})
            self.assertEqual(cached_sample('0x1234', 'weapon', False)['file'], 'samples/0x1234.flac')
            self.assertIsNone(cached_sample('0x1234', 'weapon', True), 'other settings')
            flac.unlink()
            self.assertIsNone(cached_sample('0x1234', 'weapon', False), 'its file is gone')

    def test_finished_segment_is_reused(self):
        with tempfile.TemporaryDirectory() as directory, mock.patch.object(enhance_audio, 'OUT', Path(directory)):
            base = Path(directory) / 'streams' / 'bes1'
            base.mkdir(parents=True)
            (base / '0xdd.opus').write_bytes(b'OggS')
            write_record(base / '0xdd.json', 'music', True, {'files': {'stereo': 'streams/bes1/0xdd.opus'}})
            self.assertEqual(cached_segment('bes1', '0xdd', 'music', True)['files'], {'stereo': 'streams/bes1/0xdd.opus'})

    def test_refused_replace_of_a_current_record(self):
        # Windows refuses the rename while another battle holds the record open.
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / 'shared.json'
            write_record(target, 'weapon', False, {'writer': 'first'})
            with mock.patch.object(Path, 'replace', side_effect=PermissionError(13, 'Access is denied')), \
                    mock.patch.object(enhance_audio.time, 'sleep'):
                write_record(target, 'weapon', False, {'writer': 'second'})
                self.assertEqual(json.loads(target.read_text())['report']['writer'], 'first')
                with self.assertRaises(PermissionError, msg='a stale record must be replaced'):
                    write_record(target, 'voice', False, {'writer': 'third'})
            self.assertEqual(list(Path(directory).glob('*.part')), [])

    def test_replace_retries_a_brief_refusal(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / 'shared.json'
            real = Path.replace
            refusals = iter([PermissionError(13, 'Access is denied')] * 2)

            def flaky(self, other):
                error = next(refusals, None)
                if error:
                    raise error
                return real(self, other)
            with mock.patch.object(Path, 'replace', flaky), mock.patch.object(enhance_audio.time, 'sleep'):
                write_record(target, 'weapon', False, {'writer': 'only'})
            self.assertEqual(json.loads(target.read_text())['report']['writer'], 'only')


if __name__ == '__main__':
    unittest.main()
