"""tools/audit-features.py's pure parts: seat sections, class chains, the
gap comparison and the mission-call check, on made-up data (no archives)."""
import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1] / "tools"
sys.path.insert(0, str(TOOLS))
spec = importlib.util.spec_from_file_location("audit_features", TOOLS / "audit-features.py")
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)

# A flyer like the snowspeeder: a hull with one cannon on two aimers, then a
# rear turret whose first weapon has no aimer and whose second is the cannon.
SNOWSPEEDER = [
    ("ControlSection", "BODY"), ("VehicleType", "fighter"), ("PilotPosition", "hp_pilot"), ("PilotAnimation", "drive"),
    ("WeaponName", "cannon"), ("WeaponAmmo", "0"), ("AimerNodeName", "Lcannon"), ("NextAimer", "-"), ("AimerNodeName", "Rcannon"),
    ("ControlSection", "TURRET"), ("PilotPosition", "hp_gunner"), ("PilotAnimation", "drivegunner"),
    ("YawLimits", "80 280"), ("TurretNodeName", "towcablegun"),
    ("WeaponSection", "1"), ("WeaponName", "reargun"), ("WeaponAmmo", "20"),
    ("WeaponSection", "2"), ("WeaponName", "cannon"), ("WeaponAmmo", "0"),
]
# An armed building: BODY, then TURRET1's keys, then its weapons under a
# TurretSection marker (the 2004 layout).
TURRET = [
    ("BuildingSection", "BODY"), ("GeometryName", "kam_prop_antiaircraft"), ("MaxHealth", "500"),
    ("BuildingSection", "TURRET1"), ("TurretNodeName", "aimer_y"), ("PilotPosition", "hp_main_gunner"),
    ("TurretSection", "TURRET1"), ("WeaponName", "aaa_cannon"), ("WeaponAmmo", "0"), ("AimerNodeName", "aimer_x"),
    ("BarrelNodeName", "barrel1"), ("FirePointName", "hp_fire1"), ("BarrelNodeName", "barrel2"), ("FirePointName", "hp_fire2"),
]


class SeatTests(unittest.TestCase):
    def test_flyer_sections(self):
        seats = audit.seats_of(SNOWSPEEDER)
        self.assertEqual([s["name"] for s in seats], ["BODY", "TURRET"])
        hull, turret = seats
        self.assertEqual([w["name"] for w in hull["weapons"]], ["cannon"])
        self.assertEqual(hull["weapons"][0]["aimers"], ["Lcannon", "Rcannon"])
        self.assertEqual([w["name"] for w in turret["weapons"]], ["reargun", "cannon"])
        self.assertEqual(turret["weapons"][0]["aimers"], [])
        self.assertEqual(turret["keys"]["TurretNodeName"], ["towcablegun"])
        self.assertTrue(turret["rider"])

    def test_turret_section_marker(self):
        seats = audit.seats_of(TURRET)
        self.assertEqual([s["name"] for s in seats], ["BODY", "TURRET1"])
        self.assertEqual(seats[0]["weapons"], [])
        weapon = seats[1]["weapons"][0]
        self.assertEqual(weapon["name"], "aaa_cannon")
        self.assertEqual(weapon["firePoints"], ["hp_fire1", "hp_fire2"])
        self.assertEqual(weapon["barrels"], ["barrel1", "barrel2"])
        self.assertFalse(seats[1]["rider"])

    def test_unsectioned_class_is_one_hull(self):
        seats = audit.seats_of([("WeaponName", "gun"), ("AimerNodeName", "a")])
        self.assertEqual(len(seats), 1)
        self.assertEqual(seats[0]["name"], "BODY")


class ChainTests(unittest.TestCase):
    def test_native_base_follows_the_chain(self):
        classes = {"bes_bldg_AAA_turret": {"base": "armedbuilding", "props": []},
                   "com_bldg_controlzone": {"base": "commandpost", "props": []},
                   "geo_cp": {"base": "com_bldg_controlzone", "props": []}}
        self.assertEqual(audit.native_base(classes, "bes_bldg_AAA_turret"), "armedbuilding")
        self.assertEqual(audit.native_base(classes, "geo_cp"), "commandpost")
        self.assertEqual(audit.native_base(classes, "nothing"), "nothing")

    def test_pad_era(self):
        self.assertEqual(audit.pad_era("ClassAllATK"), "gcw")
        self.assertEqual(audit.pad_era("ClassCISDEF"), "cw")
        self.assertEqual(audit.pad_era("ClassHisDEF"), "his")


class GapTests(unittest.TestCase):
    def inventory(self):
        return {
            "turrets": [{"name": "", "class": "bes_bldg_aaa_turret", "resolved": "bes_bldg_AAA_turret"}] * 2,
            "droids": [{"name": "d1", "class": "com_item_healthrecharge", "resolved": "com_item_healthrecharge"}],
            "posts": [{"name": "shield", "class": "hoth_bldg_shieldgenerator", "resolved": "hoth_bldg_shieldgenerator",
                       "destructible": True, "maxHealth": "50000.0", "wreck": "hoth_dst", "killRegion": "CP2_Death"},
                      {"name": "cp2", "class": "rep_bldg_forwardcenter", "resolved": "rep_bldg_forwardcenter",
                       "destructible": True, "maxHealth": "30000.0", "wreck": None, "killRegion": None}],
            "pads": [{"choices": {"ClassAllATK": "all_fly_xwing", "ClassRepATK": "rep_fly_gunship", "ClassHisDEF": "nab_bldg_fambaa_shield"}}],
        }

    def prepared(self):
        return {"live": {"com_item_healthrecharge": 1}, "posts": [{"name": "shield", "keys": ["name", "team"]}],
                "padChoices": [("1", "all_fly_xwing")], "sides": ["all", "imp", "wok"]}

    def test_gaps(self):
        eras = {"cw": {"extracted": False}, "gcw": {"extracted": True}}
        gaps = audit.map_gaps(self.inventory(), self.prepared(), {"all": [], "imp": []}, eras)
        kinds = [g["kind"] for g in gaps]
        self.assertEqual(kinds.count("live"), 1)
        live = next(g for g in gaps if g["kind"] == "live")
        self.assertEqual((live["class"], live["placedAs"], live["retail"], live["prepared"]), ("bes_bldg_AAA_turret", "bes_bldg_aaa_turret", 2, 0))
        self.assertEqual(kinds.count("post"), 2)
        self.assertEqual([g["name"] for g in gaps if g["kind"] == "postMissing"], ["cp2"])
        pads = {g["era"]: g for g in gaps if g["kind"] == "pads"}
        self.assertEqual(sorted(pads), ["cw", "his"])
        self.assertIs(pads["cw"]["eraExtracted"], False)
        self.assertIsNone(pads["his"]["eraExtracted"])
        self.assertEqual([g["side"] for g in gaps if g["kind"] == "side"], ["wok"])

    def test_prepared_objective_posts_and_interactive_objects(self):
        inventory = self.inventory()
        inventory["interactive"] = {"end_weap_treesmash": {"base": "trap", "placed": 2, "attached": 0},
                                    "tat1_bldg_sarlacctentacle": {"base": "animatedprop", "placed": 0, "attached": 5},
                                    "com_item_powerup_health100": {"base": "powerupitem", "placed": 1, "attached": 0}}
        prepared = {**self.prepared(), "livePosts": ["shield", "cp2"],
                    "live": {"com_item_healthrecharge": 1, "TAT1_bldg_sarlacctentacle": 5},
                    "placedItems": {"com_item_powerup_health100": 1}}
        gaps = audit.map_gaps(inventory, prepared, {"all": [], "imp": []})
        self.assertEqual([g for g in gaps if g["kind"] == "post"], [])
        interactive = [g for g in gaps if g["kind"] == "interactive"]
        self.assertEqual([(g["class"], g["retail"], g["prepared"]) for g in interactive], [("end_weap_treesmash", 2, 0)])

    def test_interactive_census_counts_attachments(self):
        classes = {"pit": {"base": "prop", "props": [("AttachOdf", "tentacle hp_tentacle1"), ("AttachOdf", "tentacle hp_tentacle2"),
                                                     ("AttachOdf", "ray hp_ray")]},
                   "tentacle": {"base": "animatedprop", "props": []},
                   "ray": {"base": "godray", "props": []},
                   "panel": {"base": "commandpost", "props": [("MaxHealth", "4000")]},
                   "flag": {"base": "commandpost", "props": []},
                   "trap": {"base": "trap", "props": []}}
        records = [{"class": "pit"}, {"class": "PANEL"}, {"class": "flag"}, {"class": "trap"}, {"class": "trap"}]
        interactive, cosmetic = audit.interactive_census(records, classes)
        self.assertEqual(interactive, {"panel": {"base": "commandpost", "placed": 1, "attached": 0},
                                       "tentacle": {"base": "animatedprop", "placed": 0, "attached": 2},
                                       "trap": {"base": "trap", "placed": 2, "attached": 0}})
        self.assertEqual(cosmetic, {"ray": {"base": "godray", "placed": 0, "attached": 1}})

    def test_unprepared_map(self):
        self.assertEqual(audit.map_gaps(self.inventory(), None, {}), [{"kind": "map", "detail": "not prepared"}])


class MissionCallTests(unittest.TestCase):
    def test_engine_consumes_looks_in_src_and_tools(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "src").mkdir()
            (root / "tools").mkdir()
            (root / "src" / "goals.js").write_text("mission.SetStayInTurrets(1); SetEnemyTeamDifficulty(x);")
            (root / "tools" / "audit-features.py").write_text("SetBirdType SetFishType")
            self.assertTrue(audit.engine_consumes("SetStayInTurrets", root))
            self.assertTrue(audit.engine_consumes("SetEnemyTeamDifficultyHard", root))
            self.assertFalse(audit.engine_consumes("SetBirdType", root))
            self.assertIsNone(audit.engine_consumes("SetBirdType", root / "elsewhere"))


if __name__ == "__main__":
    unittest.main()
