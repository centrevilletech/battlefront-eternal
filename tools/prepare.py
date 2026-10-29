#!/usr/bin/env python3
"""Prepare everything the game reads from an installation, in one run.

    BFE_GAME_DIR=<install> BFE_CONTENT_DIR=<out> python3 tools/prepare.py [--jobs N]

The desktop app runs this on first launch with its bundled toolchain
(docs/desktop.md); from a checkout it fills public/ like the separate tools.
Steps (keys in brackets), each after the ones it needs:

1. [catalog]        the front end's map catalog and loading pictures (tools/zero_shell.py),
                    the add-ons' maps included (tools/zero_addon.py)
2. [side:<side>]    every army and local force the catalog's battles field
                    (tools/extract.py --only side)
3. [map:<map>]      every map of the catalog with all its battles' sides
                    (extract.py <map> --side ... --skip-shared)
4. [common]         after the sides: what every side shares, clips, effects,
                    HUD art (extract.py --only common)
5. [shell]          after common: the menus' textures and strings, music and sounds
6. [catalog-update] after the maps: the catalog again, marking the maps extracted
7. [environment]    after the maps: the effects their placed objects carry
                    (fountains, steam, torches, smoke, mist; tools/zero_environment.py)
8. [hud]            after the shell: 4x HUD and font atlases (tools/upscale_hud.py; needs Pillow)
9. [audio:<sub>]    after its map: the Enhanced sound per battle (tools/enhance_audio.py;
                    needs numpy, scipy, soundfile and soxr, and ffmpeg)
10. [textures]      after everything that writes textures: KTX2 copies (tools/ktx2.py;
                    needs KTX-Software's `ktx`)
11. [movies]        the shell's movies, upscaled to WebM (needs ffmpeg with Bink,
                    VP9, Opus); runs alongside the rest from the start

Steps 1-8 make the game playable ("core"); the rest refine it. Steps run as
soon as what they need is done and the cores allow (--jobs): the sides and
maps several at once (as memory allows), the sound of a battle while other
maps extract, the movies throughout. The game can start once the sides,
common and shell steps and one map are done, which the "playable" event
announces. The desktop app keeps its loader up until all of them are done.
A step that completes is recorded in <out>/.prepare.json and skipped next
time, so an interrupted run resumes. The add-ons a run read are recorded
too (addOns), so one installed or removed since runs what lists them again
(ADD_ON_STEPS). A step whose tool is unavailable is skipped and reported,
not failed. One run at a time per output folder: a
second run finds <out>/.prepare.lock held, reports "busy" and exits without
touching it.

Progress goes to stdout as lines starting with "@@" followed by JSON; the
tools' own output passes through between them.
"""

import argparse
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import threading
import time
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait
from pathlib import Path

from zero_addon import add_ons, missions
from zero_paths import GAME, PUBLIC

TOOLS = Path(__file__).resolve().parent
# Re-exports the armies when it moves (stored as the state's muzzleVersion):
# 1: held weapon anchors. 2: soldiers' CollisionScale, for fall damage. 3: the
# AI class records name every property the tools know (AmbientSound, which
# 7be608c taught them without a new generation, so sides prepared since
# differed from the server's), and an add-on army's path is recorded as
# spelled on disk on every platform (tools/zero_addon.py child).
CONTENT_VERSION = json.loads((TOOLS / "content-version.json").read_text())
SIDE_EXPORT_VERSION = CONTENT_VERSION["sides"]
MAP_IMPORT_VERSION = CONTENT_VERSION["maps"]  # 1: FOLG grid/scatter and painted mesh collision rules. 2: collision filed under the model a class draws. 3: every battle of a map (both eras), vehicle effects and sounds, posts' map icons. 4: haze, dust, snow, sky models and attached effects on every map. 5: turrets' damage effects and pieces, walkers' dying explosions. 6: geometry attached to placed props' hardpoints (Jabba, the rancor, sarlacc tentacles). 7: animated props and doors live, with their clips (props_anims.bin). 8: their bone-hung collision shapes (the closed trapdoor is solid). 9: soft-skinned props keep their bones' weights (the sarlacc tentacle, Jabba, the rancor), every Animation, AnimationTrigger and AttachTrigger of an animated prop, and the death regions a battle's script names (the sarlacc pit's Sarlac01). Hover suspension springs and walker foot bottoms/rules. 10: destructible command posts live (the shield generator, the Techno Union ships, the forward command center, the bunker control panel), their death clips and kill regions, the objective map icon and Info voice-over. 11: walker aimer and barrel records as the original game groups them, leg pairs (walkers stand on their legs), FireOutsideLimits per aimer. 12: traps live (Endor's Ewok log traps: their swing and build clips, build pad and DeathEffect), no longer baked; the power-ups the world places (props.json items; Yavin 4: Arena's health power-up). 13: the same bits on every platform: spawn paths' and posts' yaws, the sun's direction, foliage and collision shapes turned by tools/portable_math.py, not the platform's sin, cos and atan2 (Windows' differ from glibc's in the last bit), and an add-on map's recorded paths spelled as on disk. 14: flyers' takeoff clips (landing gear, gear doors, the X-wing's S-foils), drawn by flight state and left out of the content fingerprint. 15: the sky's flyers (SkyObject: sky/ beside map.json, for the battles that load their models; presentation only, outside the content fingerprint, so map.json and every fingerprinted file come out as at 14). 16: the Enhanced sky's flyers: every class of a battle's era (sky/flyers.json `enhanced`, with each class's era) and their models from the side archives, Bespin: Platforms' among them; presentation only, so the fingerprinted files come out as at 15.
# What every side shares (the common step) moves with this: 2: the Hoth dish
# turret gunner's human_dishcannon_9pose clip. Stored as commonVersion.
COMMON_VERSION = CONTENT_VERSION["common"]
STATE_VERSION = CONTENT_VERSION["state"]  # 2: sides and common are steps of their own, a map is one step
# What the installed add-ons (GameData/AddOn, tools/zero_addon.py) change in
# steps that are done: the catalog lists their maps, common takes their
# armies' clips and their maps' effects, the movies their flyovers, and
# catalog-update marks their maps ready. Their maps, armies and sound are
# steps of their own.
ADD_ON_STEPS = ("catalog", "common", "catalog-update", "movies")
ERA_TEAMS = {"cw": ["rep", "cis"], "gcw": ["all", "imp"]}
SIDE_NAMES = {
    "rep": "Republic", "cis": "CIS", "all": "Rebel Alliance", "imp": "Galactic Empire",
    "geo": "Geonosians", "ewk": "Ewoks", "wok": "Wookiees", "gun": "Gungans", "des": "Tusken Raiders",
    "gar": "Garrison", "jaw": "Jawas", "gam": "Gamorreans",
}


OUTPUT = threading.Lock()


def say(line):
    with OUTPUT:
        print(line, flush=True)


def emit(kind, **fields):
    say("@@" + json.dumps({"event": kind, "time": time.time(), **fields}))


def add_on_names(game=GAME):
    """The installed add-ons' names (their folders', lower case), sorted: what
    .prepare.json's addOns records and electron/prepare.js compares."""
    return sorted(add_on.name for add_on in add_ons(game))


class State:
    def __init__(self, path, installed=None):
        self.path = path
        try:
            self.data = json.loads(path.read_text())
        except (OSError, ValueError):
            self.data = {}
        if self.data.get("version") != STATE_VERSION or self.data.get("game") != str(GAME):
            self.data = {"version": STATE_VERSION, "game": str(GAME), "done": []}
        if self.data.get("mapImportVersion") != MAP_IMPORT_VERSION:
            # Reuse armies, audio, HUD and movies; only map products and their
            # derived catalog/environment/texture stages need rebuilding.
            self.data["done"] = [key for key in self.data["done"]
                                 if not key.startswith("map:") and key not in
                                 ("catalog-update", "environment", "textures")]
            self.data["mapImportVersion"] = MAP_IMPORT_VERSION
        # Re-export the armies when what a side exports changed. Preserve
        # completed maps/audio: their output does not depend on it.
        if self.data.get("muzzleVersion") != SIDE_EXPORT_VERSION:
            self.data["done"] = [key for key in self.data["done"] if not key.startswith("side:")]
            self.data["muzzleVersion"] = SIDE_EXPORT_VERSION
        # Re-export what every side shares (its clips) when it changed; the
        # armies, maps and audio stand.
        if self.data.get("commonVersion") != COMMON_VERSION:
            self.data["done"] = [key for key in self.data["done"] if key != "common"]
            self.data["commonVersion"] = COMMON_VERSION
        # Add-ons installed or removed since (none recorded: prepared before
        # add-ons were read, as Jabba's Palace was missed): what lists them
        # runs again.
        installed = add_on_names(GAME) if installed is None else list(installed)
        if self.data.get("addOns", []) != installed:
            self.data["done"] = [key for key in self.data["done"] if key not in ADD_ON_STEPS]
            self.data["addOns"] = installed
        # A run in progress is not complete, whatever the last one recorded.
        for key in ("complete", "finished", "failed", "skipped"):
            self.data.pop(key, None)

    def done(self, key):
        return key in self.data["done"]

    def mark(self, key):
        if key not in self.data["done"]:
            self.data["done"].append(key)
        self.save()

    def save(self, **extra):
        self.data.update(extra)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        temp = self.path.with_suffix(".tmp")
        temp.write_text(json.dumps(self.data, indent=1))
        temp.replace(self.path)


def has_modules(*names):
    return all(importlib.util.find_spec(name) is not None for name in names)


def ffmpeg_ok():
    return shutil.which("ffmpeg") is not None


def ktx_ok():
    if os.environ.get("KTX2_ENCODER") == "off":
        return False
    encoder = os.environ.get("KTX2_ENCODER")
    return bool(encoder and Path(encoder).exists()) or shutil.which("ktx") is not None


def run_tool(script, *args, prefix=""):
    """Runs a tool with this interpreter; True when it succeeds. Its output
    passes through, each line led by prefix when tools run side by side."""
    command = [sys.executable, "-u", str(TOOLS / script), *map(str, args)]
    say(f"{prefix}$ " + " ".join(command[2:]))
    child = subprocess.Popen(command, cwd=TOOLS.parent, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                             text=True, encoding="utf-8", errors="replace", bufsize=1)
    for line in child.stdout:
        say(prefix + line.rstrip("\n"))
    return child.wait() == 0


def memory():
    """(total, available) bytes, or (None, None) where /proc/meminfo isn't."""
    try:
        info = dict(line.split(":", 1) for line in Path("/proc/meminfo").read_text().splitlines())
        return int(info["MemTotal"].split()[0]) * 1024, int(info["MemAvailable"].split()[0]) * 1024
    except (OSError, KeyError, ValueError):
        return None, None


def map_workers(jobs, per_gb=0.6, most=6, least=2, mem=None):
    """Processes to run at once: one per core asked for and per_gb of memory
    each (an extraction's measured need is 250 to 440 MB; 0.6 GB leaves
    room), at most most and at least least (jobs permitting), since two
    always fit and running short of memory swaps, which stalls the whole
    desktop."""
    total, available = memory() if mem is None else mem
    by_memory = int(max(available or 0, (total or 8 << 30) * 0.4) // (per_gb * (1 << 30))) if total else 4
    return max(min(least, jobs), min(jobs, by_memory, most))


def mission_sides():
    """Every side each map's mission scripts read (SIDE\\<name>.lvl), by map
    id, from mission.lvl and the add-ons'. Empty when the install or its
    scripts are missing."""
    try:
        from zero_lua import mission_setup
        scripts = missions(GAME)
    except (OSError, ImportError, ValueError):
        return {}
    sides = {}
    for name, chunk in scripts.items():
        map_id = name[:4].lower()
        try:
            found = mission_setup(chunk)["sides"]
        except Exception:  # noqa: BLE001 - one odd script must not stop the plan
            continue
        for side in found:
            if side not in sides.setdefault(map_id, []):
                sides[map_id].append(side)
    return sides


def catalog():
    try:
        return json.loads((PUBLIC / "common" / "shell" / "shell.json").read_text())["maps"]
    except (OSError, ValueError, KeyError):
        return []


def mission_sublevels(maps, locals_of):
    """Each map's sound sub-levels (the Enhanced sound's units of work), by
    map id, from the sound archive each of its missions reads
    ("sound\\bes.lvl;bes1cw"): known before any map is extracted, so the
    sound steps are in the plan from the start."""
    try:
        from zero_lua import calls, load, mission_setup
        from zero_sound import read_mission_audio
        scripts = missions(GAME)
    except (OSError, ImportError, ValueError):
        return {}
    found = {}
    for entry in maps:
        map_id = entry["id"]
        for mission in sorted((m for m in scripts if m.startswith(map_id)), key=lambda m: m.endswith("_h")):
            try:
                setup = mission_setup(scripts[mission])
                if not any(side in setup["sides"] for side in map_sides(entry, locals_of)):
                    continue
                banks = read_mission_audio(calls(load(scripts[mission])))["banks"]
            except Exception:  # noqa: BLE001 - one odd script must not stop the plan
                continue
            for bank in banks[:1]:
                sub = bank.split("\\")[-1].partition(";")[2]
                if sub and sub not in found.setdefault(map_id, []):
                    found[map_id].append(sub)
    return found


def map_sides(entry, locals_of):
    """The sides a map's battles field, in era order: its playable teams,
    then the locals its missions read (Kashyyyk's Wookiees, Mos Eisley's
    Jawas), never a playable team but loaded by the battle."""
    teams = []
    for era in entry.get("eras", {}).values():
        for team in era.get("teams") or []:
            if team not in teams:
                teams.append(team)
    for side in locals_of.get(entry["id"], []):
        if side not in teams:
            teams.append(side)
    return teams


def step(key, phase, label, weight, available, action, after=(), cores=1):
    """A step: (key, phase, label, weight, available, action, needs), needs
    being the keys (or key prefixes ending in ":") it runs after and the
    cores it keeps busy."""
    return (key, phase, label, weight, available, action, {"after": tuple(after), "cores": cores})


def needs(item):
    return item[6] if len(item) > 6 else {"after": (), "cores": 1}


def audio_jobs(jobs):
    """Workers one sound step runs: a few sound steps share the cores."""
    return max(2, min(4, jobs // 2))


def plan(jobs, maps=None, locals_of=None, sublevels=None):
    """The steps (see step()), in the order the progress list shows them;
    run_steps schedules them by their needs."""
    steps = [step("catalog", "core", "Reading the galaxy map", 2, True, lambda: run_tool("zero_shell.py", "--catalog"))]
    maps = catalog() if maps is None else maps
    locals_of = mission_sides() if locals_of is None else locals_of
    sublevels = mission_sublevels(maps, locals_of) if sublevels is None else sublevels
    # The movies are the longest step by far (55 encodes, 24 minutes of
    # video), so they are second in the plan and run throughout: an encode
    # per two cores (each has four threads and keeps about two and a half
    # cores busy, libvpx, and ~650 MB), as memory allows, counted as half
    # the cores so the rest of the plan overlaps them rather than waits.
    encodes = map_workers(max(1, jobs // 2), per_gb=0.75, most=8)
    steps.append(step("movies", "extra", "Upscaling the movies", 30, ffmpeg_ok(),
                      lambda: run_tool("zero_shell.py", "--movies", "--jobs", encodes), after=("catalog",), cores=max(1, jobs // 2)))
    sides = []
    battles = []
    for entry in maps:
        teams = map_sides(entry, locals_of)
        battles.append((entry["id"], entry.get("name") or entry["id"], teams))
        sides += [team for team in teams if team not in sides]
    for side in sides:
        steps.append(step(f"side:{side}", "core", f"{SIDE_NAMES.get(side, side.upper())} units", 3, True,
                          lambda side=side: run_tool("extract.py", "--only", "side", "--side", side, prefix=f"[{side}] "),
                          after=("catalog",)))
    if maps:
        steps += [
            step("common", "core", "What every side shares", 3, True, lambda: run_tool("extract.py", "--only", "common", prefix="[common] "),
                 after=("side:",)),
            step("shell", "core", "Building the menus", 5, True, lambda: run_tool("zero_shell.py", "--common", "--audio", "--catalog"),
                 after=("common",)),
        ]
    for map_id, name, teams in battles:
        side_args = [arg for team in teams for arg in ("--side", team)]
        steps.append(step(f"map:{map_id}", "core", name.title(), 4 + len(teams), True,
                          lambda map_id=map_id, side_args=side_args: run_tool("extract.py", map_id, *side_args, "--skip-shared",
                                                                              prefix=f"[{map_id}] "),
                          after=("catalog",)))
    if maps:
        steps.append(step("catalog-update", "core", "Marking the maps ready", 1, True, lambda: run_tool("zero_shell.py", "--catalog"),
                          after=("map:",)))
    steps += [
        # The effects the maps' placed objects carry: Theed's fountains,
        # steam vents, torches, smoke, mist (docs/environment-effects.md),
        # for every map extracted.
        step("environment", "core", "Fountains, steam and fires", 1, bool(maps),
             lambda: run_tool("zero_environment.py", *[entry["id"] for entry in maps
                                                       if (PUBLIC / "maps" / entry["id"] / "map.json").exists()]),
             after=("map:",)),
        step("hud", "core", "Sharpening the HUD", 2, has_modules("PIL"),
             lambda: run_tool("upscale_hud.py", "--common", PUBLIC / "common"), after=("shell",)),
    ]
    enhanced = has_modules("numpy", "scipy", "soundfile", "soxr") and ffmpeg_ok()
    # An era's battles share their voice-over, made once by the first of
    # them (tools/enhance_audio.py keeps it): the rest of the era follow it.
    leaders = {}
    for map_id, _, _ in battles:
        for sublevel in sublevels.get(map_id, []):
            era = "gcw" if sublevel.endswith("gcw") else "cw" if sublevel.endswith("cw") else sublevel
            after = [f"map:{map_id}"] + ([leaders[era]] if era in leaders else [])
            leaders.setdefault(era, f"audio:{sublevel}")
            steps.append(step(f"audio:{sublevel}", "extra", f"Enhancing sound: {sublevel}", 6, enhanced,
                              lambda sublevel=sublevel: run_tool("enhance_audio.py", sublevel, "--jobs", audio_jobs(jobs)),
                              after=after, cores=audio_jobs(jobs)))
    steps.append(step("textures", "extra", "Compressing textures", 8, ktx_ok(), lambda: run_tool("ktx2.py", "--jobs", audio_jobs(jobs)),
                      after=("map:", "side:", "common", "shell", "environment", "hud"), cores=audio_jobs(jobs)))
    return steps, bool(maps)


BATCHED = ("side", "map")


def batch_of(key):
    """The kind of extraction a step is (several run side by side, as
    memory allows), or None."""
    prefix = key.split(":", 1)[0]
    return prefix if ":" in key and prefix in BATCHED else None


def satisfied(dep, unfinished):
    """Whether a dependency (a key, or a prefix ending in ":" for every
    step of a kind) has no unfinished step; a step not in the plan at all
    holds nothing up."""
    if dep.endswith(":"):
        return not any(key.startswith(dep) for key in unfinished)
    return dep not in unfinished


def run_steps(steps, jobs, finish, workers=None):
    """Runs the steps as their needs and the cores allow: a step starts
    once every step it is after has finished (done or failed), while the
    cores it keeps busy fit in jobs, and never more extractions at once than
    memory allows (workers, by default map_workers). Ready steps start in
    plan order. finish(step, ok, started) is called for each as it ends."""
    pending = list(steps)
    unfinished = {item[0] for item in pending}
    running = {}
    with ThreadPoolExecutor(max_workers=max(2, jobs * 2)) as pool:
        while pending or running:
            used = sum(needs(item)["cores"] for item, _ in running.values())
            extractions = sum(1 for item, _ in running.values() if batch_of(item[0]))
            limit = map_workers(jobs) if workers is None else workers
            for item in list(pending):
                key, phase, label, weight, _, action, _ = item
                extra = needs(item)
                if not all(satisfied(dep, unfinished) for dep in extra["after"]):
                    continue
                if running and used + extra["cores"] > jobs:
                    continue
                if batch_of(key) and extractions >= limit:
                    continue
                pending.remove(item)
                used += extra["cores"]
                extractions += 1 if batch_of(key) else 0
                emit("step", key=key, label=label, phase=phase, weight=weight, status="start", parallel=len(running) + 1)
                running[pool.submit(action)] = (item, time.time())
            if not running:
                # Nothing can start: a need no step of the plan meets. Run
                # the first anyway rather than stop.
                item = pending.pop(0)
                emit("step", key=item[0], label=item[2], phase=item[1], weight=item[3], status="start", parallel=1)
                running[pool.submit(item[5])] = (item, time.time())
            done, _ = wait(running, return_when=FIRST_COMPLETED)
            for future in done:
                item, began = running.pop(future)
                try:
                    ok = future.result()
                except Exception as error:  # noqa: BLE001 - reported, the rest go on
                    say(f"[{item[0]}] {error}")
                    ok = False
                unfinished.discard(item[0])
                finish(item, ok, began)


ANNOUNCED = []


def announce_playable(state):
    """The game can start once the menus, what the sides share, and at least
    one map are in (the sides come before common). Announced once a run."""
    if ANNOUNCED:
        return
    if all(state.done(key) for key in ("shell", "common")) and any(key.startswith("map:") for key in state.data["done"]):
        ANNOUNCED.append(True)
        emit("playable")


def hold_lock(path):
    """An exclusive lock on path for this process's lifetime, or None when
    another run holds it. The OS drops it if the process dies."""
    handle = open(path, "a+b")
    try:
        if os.name == "nt":
            import msvcrt
            handle.seek(0)
            msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
        else:
            import fcntl
            fcntl.flock(handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError:
        handle.close()
        return None
    return handle


def stale_side(key):
    """A side step that finished with no unit to show for it runs again:
    every side fields some, and SIDE\\des.lvl (the Tusken Raiders and the
    Jawas) came out empty before tools/extract.py read the classes a side's
    archive defines whatever their names."""
    if not key.startswith("side:"):
        return False
    try:
        return not json.loads((PUBLIC / "sides" / key[5:] / "units.json").read_text()).get("units")
    except (OSError, ValueError):
        return True


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--jobs", type=int, default=os.cpu_count() or 2)
    parser.add_argument("--core-only", action="store_true", help="stop once the game is playable")
    args = parser.parse_args()

    if not (GAME / "SIDE").exists():
        emit("error", message=f"No Star Wars Battlefront installation at {GAME.parents[1]}")
        return 2
    PUBLIC.mkdir(parents=True, exist_ok=True)
    lock = hold_lock(PUBLIC / ".prepare.lock")
    if lock is None:
        emit("busy", message=f"Another preparation is already running in {PUBLIC}")
        return 3
    state = State(PUBLIC / ".prepare.json")
    failed = []
    skipped = []

    # The catalog decides which maps there are, so the plan is made again
    # after it runs; finished steps are skipped either way.
    steps, _ = plan(args.jobs)
    for attempt in range(2):
        if attempt:
            steps, has_maps = plan(args.jobs)
            if not has_maps:
                emit("error", message="The game's map catalog could not be read.")
                return 1
        emit("plan", total=sum(item[3] for item in steps), steps=[
            {"key": k, "phase": p, "label": l, "weight": w, "available": a} for k, p, l, w, a, *_ in steps])
        def runnable(item):
            key, phase, label, weight, available, *_ = item
            if attempt == 0 and key != "catalog":
                return False
            if args.core_only and phase != "core":
                return False
            if state.done(key) and not stale_side(key):
                emit("step", key=key, label=label, phase=phase, weight=weight, status="done", cached=True)
                return False
            if not available:
                if attempt:
                    skipped.append(key)
                emit("step", key=key, label=label, phase=phase, weight=weight, status="unavailable")
                return False
            return True

        def finish(item, ok, started):
            key, phase, label, weight, *_ = item
            if ok:
                state.mark(key)
            else:
                failed.append(key)
            emit("step", key=key, label=label, phase=phase, weight=weight, status="done" if ok else "failed",
                 seconds=round(time.time() - started, 1))

        def finish_announcing(item, ok, started):
            finish(item, ok, started)
            if attempt:
                announce_playable(state)

        run_steps([item for item in steps if runnable(item)], args.jobs, finish_announcing)
        if attempt:
            announce_playable(state)

    core_failed = [key for key in failed if key in ("catalog", "shell", "common")]
    # Complete only after the full plan: nothing failed, nothing skipped for a
    # missing tool, and not stopped at --core-only. The app re-runs otherwise,
    # where finished steps are skipped.
    state.save(finished=time.time(), failed=failed, skipped=skipped, complete=not args.core_only)
    emit("finished", failed=failed)
    return 1 if core_failed else 0


if __name__ == "__main__":
    sys.exit(main())
