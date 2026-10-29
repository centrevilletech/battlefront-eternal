"""Spawn intake must follow authored post references, not a map-name convention."""
import sys
import unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from extract import command_post_paths


class MapSpawnTests(unittest.TestCase):
    def test_both_native_naming_conventions_and_casing(self):
        old = [[1, 2, 3, 0]]
        kamino = [[4, 5, 6, 1], [7, 8, 9, 2]]
        paths = {'CP1Spawn': old, 'CP2SpawnPath': kamino, 'boundary1': [[99, 0, 99, 0]],
                 'unusedSpawn': [[50, 0, 50, 0]]}
        posts = [{'spawnPath': 'CP1Spawn'}, {'spawnPath': 'cp2spawnpath'}, {'spawnPath': None}]
        result = command_post_paths(paths, posts)
        self.assertEqual(result, {'CP1Spawn': old, 'cp2spawnpath': kamino})
        self.assertIs(result['cp2spawnpath'], kamino)

    def test_missing_reference_does_not_substitute_patrol_points(self):
        self.assertEqual(command_post_paths({'boundary1': [[1, 2, 3, 0]]},
                                           [{'spawnPath': 'missing'}]), {})


if __name__ == '__main__':
    unittest.main()
