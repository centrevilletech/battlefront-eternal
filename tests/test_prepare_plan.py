"""tools/prepare.py's plan: the sides and common before the maps, each map
one step with all its sides, batches that never run one key twice at once,
and workers sized by measured need."""
import os
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
import prepare  # noqa: E402


class LockHandoverTests(unittest.TestCase):
    def test_windows_runs_take_their_bytes_by_name_and_check_a_handed_lock(self):
        """Windows' decisions, with byte-range locking simulated: a 2005 run
        holds bytes 0 and 1, a handed-over run takes byte 1 in its own name
        once standard input is the lock file, and a 2004 run is unchanged."""
        from types import SimpleNamespace
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as directory:
            lock = Path(directory) / ".prepare.lock"
            lock.write_bytes(b"")
            other = Path(directory) / "other"
            other.write_bytes(b"")
            held, mine = set(), set()

            def locking(fd, mode, count):
                byte = os.lseek(fd, 0, os.SEEK_CUR)
                self.assertEqual(count, 1)
                if mode == msvcrt.LK_UNLCK:
                    mine.discard(byte)
                elif byte in held or byte in mine:
                    raise OSError(33, "locked")
                else:
                    mine.add(byte)
            msvcrt = SimpleNamespace(LK_NBLCK=2, LK_UNLCK=0, locking=locking)

            def run(family, handed=False, stdin=lock, held_bytes=(), identity=True):
                held.clear()
                held.update(held_bytes)
                mine.clear()
                real_fstat = os.fstat
                given = None
                if stdin:
                    fd = os.open(stdin, os.O_RDONLY)
                    given = real_fstat(fd)
                    os.close(fd)

                def fstat(fd):
                    if fd == 0:
                        if given is None:
                            raise OSError(9, "no standard input")
                        return given if identity else os.stat_result((given.st_mode, 0, *given[2:10]))
                    return real_fstat(fd)
                env = {"BFE_PREPARE_LOCK": "stdin"} if handed else {}
                with (patch.dict(sys.modules, {"msvcrt": msvcrt}), patch.object(os, "name", "nt"),
                      patch.object(os, "fstat", fstat), patch.object(os, "dup2") as dup2,
                      patch.object(prepare.zero_family, "is_swbf2", return_value=family == "swbf2"),
                      patch.dict(os.environ, env, clear=False)):
                    if not handed:
                        os.environ.pop("BFE_PREPARE_LOCK", None)
                    try:
                        handle = prepare.hold_lock(lock)
                    except prepare.HandoverUnconfirmed:
                        return "unconfirmed", set(mine), dup2.called
                if handle is not None:
                    handle.close()
                return ("busy" if handle is None else "held"), set(mine), dup2.called

            # By name: a 2005 run holds both bytes, a 2004 run byte 0 as released.
            self.assertEqual(run("swbf2"), ("held", {0, 1}, False))
            self.assertEqual(run("swbf1"), ("held", {0}, False))
            # A handed-over run whose client ended still holds byte 1: busy, byte 0 let go.
            self.assertEqual(run("swbf2", held_bytes={1})[0], "busy")
            # Handed over: the client holds byte 0 and has let go of byte 1.
            self.assertEqual(run("swbf2", handed=True, held_bytes={0}), ("held", {1}, True))
            self.assertEqual(run("swbf2", handed=True, held_bytes={0, 1}), ("busy", set(), False))
            self.assertEqual(run("swbf2", handed=True, stdin=other, held_bytes={0}), ("busy", set(), False))
            self.assertEqual(run("swbf2", handed=True, stdin=None, held_bytes={0}), ("unconfirmed", set(), False))
            self.assertEqual(run("swbf2", handed=True, held_bytes={0}, identity=False), ("unconfirmed", set(), False))
            # A 2004 run ignores the handover.
            self.assertEqual(run("swbf1", handed=True, held_bytes={0}), ("busy", set(), False))

    @unittest.skipUnless(os.name == "posix", "uses flock on inherited file descriptions")
    def test_helpers_started_after_a_handover_do_not_keep_the_lock(self):
        import fcntl
        import signal
        with tempfile.TemporaryDirectory() as directory:
            lock = Path(directory) / ".prepare.lock"
            child = """import os, subprocess, sys
from pathlib import Path
from prepare import hold_lock
h = hold_lock(Path(sys.argv[1]))
helper = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"],
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
print(helper.pid, h is not None, os.path.samestat(os.fstat(0), os.stat(os.devnull)), flush=True)
"""
            env = dict(os.environ, BFE_FAMILY="swbf2", BFE_PREPARE_LOCK="stdin",
                       PYTHONPATH=str(Path(prepare.__file__).parent))
            owner = lock.open("a+b")
            fcntl.flock(owner, fcntl.LOCK_EX)
            pid, adopted, null = subprocess.check_output([sys.executable, "-c", child, str(lock)],
                                                         stdin=owner, env=env, text=True).split()
            try:
                # The client and the run are gone; the helper still runs and holds no copy of the lock.
                owner.close()
                with lock.open("a+b") as probe:
                    try:
                        fcntl.flock(probe, fcntl.LOCK_EX | fcntl.LOCK_NB)
                        free = True
                    except BlockingIOError:
                        free = False
                self.assertTrue(free, "a helper the run started kept the lock")
                self.assertEqual((adopted, null), ("True", "True"))
            finally:
                os.kill(int(pid), signal.SIGTERM)

    @unittest.skipUnless(os.name == "posix", "requires flock")
    def test_writer_keeps_the_lock_when_the_client_closes_its_descriptor(self):
        import fcntl
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / ".prepare.lock"
            owner = path.open("a+b")
            self.addCleanup(owner.close)
            fcntl.flock(owner, fcntl.LOCK_EX)
            script = """import os, sys
from pathlib import Path
from prepare import hold_lock
writer = hold_lock(Path(sys.argv[1]))
print(writer is not None, flush=True)
os.read(int(sys.argv[2]), 1)
"""
            env = dict(os.environ, BFE_FAMILY="swbf2", BFE_PREPARE_LOCK="stdin",
                       PYTHONPATH=str(Path(prepare.__file__).parent))
            r, w = os.pipe()
            with os.fdopen(r, "rb") as wait, os.fdopen(w, "wb", buffering=0) as release:
                with subprocess.Popen([sys.executable, "-c", script, str(path), str(wait.fileno())],
                                      stdin=owner, stdout=subprocess.PIPE, text=True, env=env,
                                      pass_fds=(wait.fileno(),)) as writer:
                    try:
                        self.assertEqual(writer.stdout.readline().strip(), "True")
                        owner.close()
                        with path.open("a+b") as probe, self.assertRaises(BlockingIOError):
                            fcntl.flock(probe, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    finally:
                        release.close()
            with path.open("a+b") as probe:
                fcntl.flock(probe, fcntl.LOCK_EX | fcntl.LOCK_NB)

    @unittest.skipUnless(os.name == "posix", "uses flock on inherited file descriptions")
    def test_only_the_2005_owner_can_hand_its_lock_to_the_child(self):
        import fcntl
        with tempfile.TemporaryDirectory() as directory:
            lock = Path(directory) / ".prepare.lock"
            child = """import os, sys
from pathlib import Path
from prepare import hold_lock
h = hold_lock(Path(sys.argv[1]))
print('busy' if h is None else os.lseek(h.fileno(), 0, os.SEEK_CUR))
"""
            def enter(stdin, family="swbf2", marker="stdin"):
                env = dict(os.environ, BFE_FAMILY=family, PYTHONPATH=str(Path(prepare.__file__).parent))
                env.pop("BFE_PREPARE_LOCK", None)
                if marker:
                    env["BFE_PREPARE_LOCK"] = marker
                return subprocess.check_output([sys.executable, "-c", child, str(lock)],
                                               stdin=stdin, env=env, text=True).strip()

            with lock.open("a+b") as owner, lock.open("rb") as stranger:
                owner.seek(17)
                self.assertEqual(enter(owner), "0")  # Unlocked stdin: take the lock by name.
                fcntl.flock(owner, fcntl.LOCK_EX)
                self.assertEqual(enter(owner), "17")
                self.assertEqual(enter(stranger), "busy")
                self.assertEqual(enter(subprocess.DEVNULL), "busy")
                self.assertEqual(enter(owner, family="swbf1"), "busy")
                self.assertEqual(enter(owner, marker=None), "busy")
                fcntl.flock(owner, fcntl.LOCK_UN)
                self.assertEqual(enter(subprocess.DEVNULL), "0")

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

    def test_no_step_converts_the_movies(self):
        # The client plays the Bink files from the game's archives; the catalog step lists them.
        keys, steps = self.keys()
        self.assertEqual([k for k in keys if "movie" in k], [])
        self.assertEqual([k for k in prepare.ADD_ON_STEPS if "movie" in k], [])
        calls = []
        original = prepare.run_tool
        prepare.run_tool = lambda script, *args, prefix="": calls.append((script, [str(a) for a in args])) or True
        try:
            steps[0][5]()
        finally:
            prepare.run_tool = original
        self.assertEqual(calls, [("zero_shell.py", ["--catalog"])])

    def test_sides_once_then_common_shell_then_maps_then_catalog_update(self):
        keys, steps = self.keys()
        self.assertEqual(keys[0], "catalog")
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
        steps, has_maps = prepare.plan(8, maps=[], locals_of={})
        self.assertFalse(has_maps)
        self.assertEqual([k for k, *_ in steps if k in ("common", "shell", "catalog-update")], [])

    def test_an_add_ons_steps_name_its_maps_and_the_games_none(self):
        maps = MAPS + [{"id": "abc1", "name": "ABC: Test", "eras": {"cw": {"teams": ["rep", "cis"]}}}]
        locals_of = {**LOCALS, "abc1": ["abc"]}
        steps, _ = prepare.plan(8, maps=maps, locals_of=locals_of, sublevels={"abc1": ["abc1cw"], "geo1": ["geo1cw"]},
                                add_on_maps={"abc1"})
        owners = {item[0]: prepare.needs(item)["maps"] for item in steps}
        self.assertEqual(owners["map:abc1"], ("abc1",))
        self.assertEqual(owners["side:abc"], ("abc1",), "an army only the add-on fields is the add-on's")
        self.assertEqual(owners["side:rep"], (), "the game's own armies stay the game's")
        self.assertEqual(owners["audio:abc1cw"], ("abc1",))
        self.assertEqual(owners["audio:geo1cw"], ())
        for key in ("catalog", "common", "shell", "map:geo1", "environment", "catalog-update"):
            self.assertEqual(owners[key], ())

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
        # After the armies too: one only add-on maps field that fails leaves them out.
        self.assertEqual(after["catalog-update"], ("map:", "side:"))
        self.assertEqual(after["environment"], ("map:", "side:"))
        self.assertEqual(after["hud"], ("shell",))
        self.assertEqual(after["audio:bes1cw"], ("catalog-update",))
        self.assertEqual(after["audio:bes1gcw"], ("catalog-update",))
        self.assertEqual(after["audio:geo1cw"], ("catalog-update", "audio:bes1cw"))
        self.assertIn("map:", after["textures"])
        self.assertEqual(prepare.needs(steps[0])["cores"], 1)
        self.assertEqual({item[0]: prepare.needs(item)["cores"] for item in steps if item[0] in ("textures", "audio:bes1cw")},
                         {"textures": 4, "audio:bes1cw": 4})
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
            ("common", ("side:",), 1, True), ("hud", ("catalog",), 2, True), ("audio:x", ("map:x",), 2, True),
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
