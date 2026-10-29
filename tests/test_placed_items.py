"""tools/zero_placed_items.py: the world's placed power-ups, on made-up
records (no archives; the class reader is stubbed)."""
import sys
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
import zero_placed_items  # noqa: E402


class PlacedItemTests(unittest.TestCase):
    def test_only_power_up_items_with_their_class(self):
        classes = {"com_item_powerup_health100": {"base": "powerupitem", "props": []},
                   "com_item_healthrecharge": {"base": "powerupstation", "props": []},
                   "yav2_bldg_arch": {"base": "prop", "props": []}}
        records = [{"class": "com_item_powerup_health100", "name": "", "props": [("Team", "0"), ("Layer", "2")], "xfrm": [1] * 12},
                   {"class": "com_item_healthrecharge", "name": "d1", "props": [], "xfrm": [0] * 12},
                   {"class": "yav2_bldg_arch", "name": "", "props": [], "xfrm": [0] * 12},
                   {"class": "missing", "name": "", "props": [], "xfrm": [0] * 12}]
        item = {"class": "com_item_powerup_health100", "base": "powerupitem", "PersonHealth": 75.0, "LifeSpan": 30.0}
        with mock.patch.object(zero_placed_items, "item_class", return_value=item) as reader:
            found = zero_placed_items.placed_items(records, classes, ["archive"])
        reader.assert_called_once_with(["archive"], "com_item_powerup_health100")
        self.assertEqual(found, [{"class": "com_item_powerup_health100", "name": "", "team": 0, "xfrm": [1] * 12, "item": item}])

    def test_unreadable_class_is_skipped(self):
        classes = {"x": {"base": "PowerupItem", "props": []}}
        with mock.patch.object(zero_placed_items, "item_class", return_value=None):
            self.assertEqual(zero_placed_items.placed_items([{"class": "x", "name": "", "props": [], "xfrm": []}], classes, []), [])


if __name__ == "__main__":
    unittest.main()
