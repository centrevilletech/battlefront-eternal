#!/usr/bin/env python3
"""Run the Python tests of the preparation and packaging tools (tests/test_*.py).

Two test modules need optional packages that only the optional tools use (see
tools/requirements-audio.txt and the vector HUD tool). They are skipped, by
name, when their package is not installed; every other module must load and
pass. Nothing here reads original game files."""
import importlib.util
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TESTS = ROOT / "tests"
OPTIONAL = {"test_audio_cache": "soundfile", "test_vector_hud": "vtracer"}


def main() -> int:
    sys.path.insert(0, str(TESTS))
    loader = unittest.TestLoader()
    suite = unittest.TestSuite()
    for path in sorted(TESTS.glob("test_*.py")):
        package = OPTIONAL.get(path.stem)
        if package and importlib.util.find_spec(package) is None:
            print(f"skipping {path.stem}: the optional package {package} is not installed")
            continue
        suite.addTests(loader.loadTestsFromName(path.stem))
    result = unittest.TextTestRunner(verbosity=1).run(suite)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main())
