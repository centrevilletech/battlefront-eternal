"""Concurrent battles publish shared enhancement metadata atomically."""
import json
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
