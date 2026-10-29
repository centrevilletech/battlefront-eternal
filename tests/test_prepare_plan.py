"""tools/prepare.py's plan: the sides and common before the maps, each map
one step with all its sides, batches that never run one key twice at once,
and workers sized by measured need."""
import sys
import threading
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
import prepare  # noqa: E402

MAPS = [
    {"id": "bes1", "name": "Bespin: Platforms", "eras": {"cw": {"teams": ["rep", "cis"]}, "gcw": {"teams": ["all", "imp"]}}},
    {"id": "geo1", "name": "Geonosis: Spire", "eras": {"cw": {"teams": ["rep", "cis"]}}},
    {"id": "end1", "name": "Endor: Bunker", "eras": {"gcw": {"teams": ["all", "imp"]}}},
]
LOCALS = {"geo1": ["geo"], "end1": ["ewk"]}


class PlanTests(unittest.TestCase):
    def keys(self):
        steps, has_maps = prepare.plan(8, maps=MAPS, locals_of=LOCALS, sublevels={})
        self.assertTrue(has_maps)
        return [step[0] for step in steps], steps

    def test_sides_once_then_common_shell_then_maps_then_catalog_update(self):
        keys, steps = self.keys()
        self.assertEqual(keys[:2], ["catalog", "movies"])
        sides = [k for k in keys if k.startswith("side:")]
        self.assertEqual(sides, ["side:rep", "side:cis", "side:all", "side:imp", "side:geo", "side:ewk"])
        maps = [k for k in keys if k.startswith("map:")]
        self.assertEqual(maps, ["map:bes1", "map:geo1", "map:end1"])
        order = [keys.index(k) for k in ("catalog", "side:rep", "side:ewk", "common", "shell", "map:bes1", "map:end1", "catalog-update", "environment", "hud")]
        self.assertEqual(order, sorted(order))
        self.assertTrue(keys.index("textures") > keys.index("catalog-update"))
        self.assertEqual(keys[-1], "textures")
        for item in steps:
            self.assertEqual(len(item), 7)

    def test_map_step_runs_the_extractor_once_with_every_side_and_shared_work_skipped(self):
        _, steps = self.keys()
        calls = []
        original = prepare.run_tool
        prepare.run_tool = lambda script, *args, prefix="": calls.append((script, [str(a) for a in args])) or True
        try:
            for step in steps:
                if step[0] in ("map:bes1", "map:end1", "side:ewk", "common"):
                    step[5]()
        finally:
            prepare.run_tool = original
        self.assertEqual(calls, [
            ("extract.py", ["--only", "side", "--side", "ewk"]),
            ("extract.py", ["--only", "common"]),
            ("extract.py", ["bes1", "--side", "rep", "--side", "cis", "--side", "all", "--side", "imp", "--skip-shared"]),
            ("extract.py", ["end1", "--side", "all", "--side", "imp", "--side", "ewk", "--skip-shared"]),
        ])

    def test_core_phase_and_the_plan_without_a_catalog(self):
        _, steps = self.keys()
        core = [k for k, phase, *_ in steps if phase == "core"]
        self.assertTrue(all(k in core for k in ("catalog", "side:rep", "common", "shell", "map:bes1", "catalog-update", "hud")))
        self.assertNotIn("movies", core)
        steps, has_maps = prepare.plan(8, maps=[], locals_of={})
        self.assertFalse(has_maps)
        self.assertEqual([k for k, *_ in steps if k in ("common", "shell", "catalog-update")], [])

    def test_map_sides_keep_era_order_then_locals(self):
        self.assertEqual(prepare.map_sides(MAPS[0], LOCALS), ["rep", "cis", "all", "imp"])
        self.assertEqual(prepare.map_sides(MAPS[2], LOCALS), ["all", "imp", "ewk"])

    def test_needs(self):
        steps, _ = prepare.plan(8, maps=MAPS, locals_of=LOCALS, sublevels={"bes1": ["bes1cw", "bes1gcw"], "geo1": ["geo1cw"]})
        after = {item[0]: prepare.needs(item)["after"] for item in steps}
        self.assertEqual(after["side:rep"], ("catalog",))
        self.assertEqual(after["map:bes1"], ("catalog",))
        self.assertEqual(after["common"], ("side:",))
        self.assertEqual(after["shell"], ("common",))
        self.assertEqual(after["catalog-update"], ("map:",))
        self.assertEqual(after["hud"], ("shell",))
        self.assertEqual(after["audio:bes1cw"], ("map:bes1",))
        self.assertEqual(after["audio:bes1gcw"], ("map:bes1",))
        self.assertEqual(after["audio:geo1cw"], ("map:geo1", "audio:bes1cw"))  # the era's voice-over is made once
        self.assertEqual(after["movies"], ("catalog",))
        self.assertIn("map:", after["textures"])
        self.assertEqual(prepare.needs(steps[0])["cores"], 1)
        self.assertEqual({item[0]: prepare.needs(item)["cores"] for item in steps if item[0] in ("textures", "audio:bes1cw", "movies")},
                         {"textures": 4, "audio:bes1cw": 4, "movies": 4})
        self.assertEqual(prepare.batch_of("side:rep"), "side")
        self.assertEqual(prepare.batch_of("map:bes1"), "map")
        for key in ("catalog", "common", "shell", "audio:bes1cw", "catalog-update"):
            self.assertIsNone(prepare.batch_of(key))

    def test_workers_sized_by_memory_with_a_floor_of_two(self):
        gb = 1 << 30
        self.assertEqual(prepare.map_workers(8, mem=(16 * gb, 3.6 * gb)), 6)  # 6 by memory, at most 6
        self.assertEqual(prepare.map_workers(8, mem=(4 * gb, 2 * gb)), 3)  # 2 GB free (and 40% of the total) at 0.6 each
        self.assertEqual(prepare.map_workers(8, mem=(4 * gb, 0.2 * gb)), 2)  # floor
        self.assertEqual(prepare.map_workers(1, mem=(4 * gb, 0.2 * gb)), 1)  # jobs permitting
        self.assertEqual(prepare.map_workers(4, mem=(16 * gb, 8 * gb)), 4)
        self.assertEqual(prepare.map_workers(8, mem=(None, None)), 4)


class ImportMigrationTests(unittest.TestCase):
    def test_old_maps_are_rebuilt_without_reextracting_armies_or_audio(self):
        import json
        import tempfile
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / '.prepare.json'
            preserved = ['catalog', 'side:rep', 'common', 'shell', 'hud', 'movies', 'audio:yav1cw']
            path.write_text(json.dumps({'version': prepare.STATE_VERSION, 'game': str(prepare.GAME), 'addOns': prepare.add_on_names(),
                'done': preserved + ['map:yav1', 'catalog-update', 'environment', 'textures'], 'complete': True, 'muzzleVersion': prepare.SIDE_EXPORT_VERSION, 'commonVersion': prepare.COMMON_VERSION}))
            state = prepare.State(path)
            self.assertEqual(state.data['done'], preserved)
            self.assertEqual(state.data['mapImportVersion'], prepare.MAP_IMPORT_VERSION)
            state.mark('map:yav1')
            resumed = prepare.State(path)
            self.assertTrue(resumed.done('map:yav1'))

    def test_old_muzzles_rebuild_only_side_steps(self):
        import json
        import tempfile
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / '.prepare.json'
            path.write_text(json.dumps({'version': prepare.STATE_VERSION, 'game': str(prepare.GAME), 'addOns': prepare.add_on_names(),
                'mapImportVersion': prepare.MAP_IMPORT_VERSION, 'commonVersion': prepare.COMMON_VERSION,
                'done': ['side:rep', 'side:imp', 'map:yav1', 'audio:yav1cw', 'common']}))
            state = prepare.State(path)
            self.assertEqual(state.data['done'], ['map:yav1', 'audio:yav1cw', 'common'])
            state.mark('side:rep')
            self.assertTrue(prepare.State(path).done('side:rep'))

    def test_old_common_rebuilds_only_the_common_step(self):
        import json
        import tempfile
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / '.prepare.json'
            path.write_text(json.dumps({'version': prepare.STATE_VERSION, 'game': str(prepare.GAME), 'addOns': prepare.add_on_names(),
                'mapImportVersion': prepare.MAP_IMPORT_VERSION, 'muzzleVersion': prepare.SIDE_EXPORT_VERSION,
                'done': ['side:rep', 'common', 'shell', 'hud', 'map:yav1', 'audio:yav1cw']}))
            state = prepare.State(path)
            self.assertEqual(state.data['done'], ['side:rep', 'shell', 'hud', 'map:yav1', 'audio:yav1cw'])
            self.assertEqual(state.data['commonVersion'], prepare.COMMON_VERSION)
            state.mark('common')
            self.assertTrue(prepare.State(path).done('common'))


class SchedulerTests(unittest.TestCase):
    def run_plan(self, steps, jobs, workers=None):
        lock = threading.Lock()
        running = set()
        log = []
        finished = []
        cores = {}

        def action(key, ok=True):
            def run():
                with lock:
                    running.add(key)
                    log.append(("start", key, set(running), sum(cores[k] for k in running)))
                time.sleep(0.03)
                with lock:
                    running.discard(key)
                return ok
            return run

        plan = []
        for key, after, n, ok in steps:
            cores[key] = n
            plan.append(prepare.step(key, "core", key, 1, True, action(key, ok), after=after, cores=n))
        original = prepare.emit
        prepare.emit = lambda *a, **k: None
        try:
            prepare.run_steps(plan, jobs, lambda item, ok, started: finished.append((item[0], ok)), workers=workers)
        finally:
            prepare.emit = original
        return log, finished

    def test_needs_cores_and_extraction_limit_are_respected(self):
        log, finished = self.run_plan([
            ("catalog", (), 1, True), ("side:a", ("catalog",), 1, True), ("side:b", ("catalog",), 1, False),
            ("map:x", ("catalog",), 1, True), ("map:y", ("catalog",), 1, True), ("map:z", ("catalog",), 1, True),
            ("common", ("side:",), 1, True), ("movies", ("catalog",), 2, True), ("audio:x", ("map:x",), 2, True),
            ("textures", ("map:", "common"), 4, True),
        ], jobs=4, workers=2)
        started = [entry[1] for entry in log]
        self.assertEqual(started[0], "catalog")
        self.assertLess(started.index("side:a"), started.index("common"))
        self.assertLess(started.index("side:b"), started.index("common"))  # a failed need still counts as finished
        self.assertLess(started.index("map:x"), started.index("audio:x"))
        self.assertEqual(started[-1], "textures")
        for _, key, concurrent, busy in log:
            self.assertLessEqual(busy, 4, (key, concurrent))
            self.assertLessEqual(sum(1 for k in concurrent if prepare.batch_of(k)), 2, (key, concurrent))
            if key == "common":
                self.assertFalse({"side:a", "side:b"} & concurrent)
            if key == "textures":
                self.assertEqual(concurrent, {"textures"})
        self.assertEqual(sorted(finished), sorted([(k, k != "side:b") for k in started]))

    def test_a_need_nothing_meets_does_not_stop_the_run(self):
        log, finished = self.run_plan([("a", ("ghost",), 1, True), ("b", ("a",), 1, True)], jobs=2)
        self.assertEqual([entry[1] for entry in log], ["a", "b"])
        self.assertEqual(finished, [("a", True), ("b", True)])

    def test_satisfied(self):
        self.assertTrue(prepare.satisfied("map:", {"side:a"}))
        self.assertFalse(prepare.satisfied("map:", {"map:a"}))
        self.assertFalse(prepare.satisfied("common", {"common"}))
        self.assertTrue(prepare.satisfied("map:nab2", {"map:bes1"}))


if __name__ == "__main__":
    unittest.main()
