"""Turret sprite dependencies must not depend on the currently played side."""
import sys
import unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from extract import weapon_texture_dependencies


class WeaponTextures(unittest.TestCase):
    def test_map_turret_includes_its_bolt_and_glow(self):
        self.assertEqual(weapon_texture_dependencies({
            'IconTexture': 'turret_icon', 'ReticuleTexture': 'turret_reticle',
            'ScopeTexture': 'turret_scope', 'ordnance': {'LaserTexture': 'redlaser_D'},
        }), {'turret_icon', 'turret_reticle', 'turret_scope', 'redlaser_D', 'lightglow'})

    def test_charged_and_strike_projectiles_are_included(self):
        self.assertEqual(weapon_texture_dependencies({
            'ordnance': {'GeometryName': 'rocket'},
            'charges': [{'ordnance': {'LaserTexture': 'charged_bolt'}}],
            'strike': {'LaserTexture': 'orbital_bolt'},
        }), {'charged_bolt', 'orbital_bolt', 'lightglow'})


if __name__ == '__main__':
    unittest.main()
