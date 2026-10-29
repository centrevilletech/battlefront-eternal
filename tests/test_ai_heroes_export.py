"""Synthetic melee-class extraction; no retail fixtures are tracked."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from zero_weapons import melee_properties


class MeleeExportTests(unittest.TestCase):
    def test_ordered_swings_default_hit_and_three_entry_cap(self):
        got = melee_properties([
            ("SwingTime", "0.7 0.5 swing1"),
            ("SwingTime", "1.2 -1 swing2"),
            ("SwingTime", "2"),
            ("SwingTime", "9 8 dropped"),
        ])
        self.assertEqual(got["swings"], [
            {"duration": .7, "hit": .5, "sound": "swing1"},
            {"duration": 1.2, "hit": .6, "sound": "swing2"},
            {"duration": 2, "hit": 1, "sound": None},
        ])

    def test_stock_omission_profile_is_not_the_prototype_default(self):
        got = melee_properties([])
        self.assertIsNone(got["canDeflectAuthored"])
        self.assertEqual(got["CanDeflect"], 0)
        self.assertEqual(got["deflectionProfile"], "bf1-cloned-melee-cleared-v1")
        self.assertEqual(got["swings"], [{"duration": 1, "hit": .5, "sound": None}])

    def test_explicit_can_deflect_uses_integer_prefix_last_value_wins(self):
        for text, expected in [("0", 0), ("1", 1), ("-1", 1), ("2", 1), ("true", 0), ("false", 0), ("  -2text", 1), (".5", 0)]:
            got = melee_properties([("CanDeflect", "1"), ("CanDeflect", text)])
            self.assertEqual(got["CanDeflect"], expected, text)
            self.assertEqual(got["deflectionProfile"], "authored")


if __name__ == "__main__":
    unittest.main()
