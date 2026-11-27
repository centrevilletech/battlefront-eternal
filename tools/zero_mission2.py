"""Asset requests recorded while running Battlefront II mission setup."""

import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import tempfile

import zero_outputs2 as products

from zero_family import read_mission_manifest
from zero_paths import GAME


PREPARED_VERSION = json.loads(Path(__file__).with_name("content-version.json").read_bytes())["swbf2"]
OPENING_MISSIONS = {"tat2": "tat2g_eli", "spa6": "spa6c_ass"}


def opening_manifest(map_id):
    return read_mission_manifest(GAME / "mission.lvl", OPENING_MISSIONS[map_id])


def layer_name_parts(mission_archive):
    """Read identifier constants used to name worlds and roles in installed PC scripts."""
    from zero_lua import mission_scripts
    identifiers = set()
    if mission_archive.is_file():
        for body in mission_scripts(mission_archive.read_bytes()).values():
            if not body.startswith(b"\x1bLua\x50\x01\x04\x04"):
                continue
            # Lua 5.0 string constants have type 4 and a size including the terminator.
            for size, value in re.findall(rb"\x04(.{4})([A-Za-z0-9_]+)\x00", body, re.DOTALL):
                if int.from_bytes(size, "little") == len(value) + 1:
                    identifiers.add(value.decode("ascii").lower())
    suffixes = {m[1] for name in identifiers if (m := re.fullmatch(r"[a-z]+[0-9]+_([a-z0-9]+)", name))}
    roles = {name.split("_")[1] for name in identifiers if name.count("_") >= 2}
    return suffixes, roles & identifiers


def data_files(manifest):
    """Ordered (archive path, named subarchives) requests."""
    for call in (manifest or {}).get("calls", []):
        if call.get("name", call.get("function")) != "ReadDataFile":
            continue
        args = call["args"]
        path, _, suffix = args[0].replace("\\", "/").lower().partition(";")
        names = ([suffix] if suffix else []) + [s.lower() for s in args[1:] if isinstance(s, str)]
        yield path, names


def mission_assets(manifest, map_id):
    layers, sides, sounds = [], {}, []
    sky = None
    for path, names in data_files(manifest):
        archive = PurePosixPath(path)
        if archive.stem == map_id and archive.parent.name == map_id[:3]:
            layers.extend(names)
        elif archive.parent.name == "side":
            sides.setdefault(archive.stem, []).extend(names)
        elif archive.parent.name == "sound":
            sounds.extend(names)
        elif archive.name == "spa_sky.lvl" and names:
            sky = names[0]
    return {"layers": layers, "sides": sides, "sky": sky, "sound": sounds}


def map_products_current(directory):
    from zero_class_art2 import catalog_current
    from zero_vehicle2 import pack_current
    try:
        record = json.loads((directory / "map.json").read_bytes())
        return (all(isinstance(section.get(key), list) for section in [record, *record["layers"].values()]
                    for key in ("barriers", "hints")) and pack_current(directory, record)
                and catalog_current(directory / "hud"))
    except (OSError, ValueError, KeyError, TypeError, AttributeError):
        return False


def prepare_opening(args, *, initializing=False):
    """Prepare both missions together so an army shared by them has both rosters."""
    from prepare import Ran, audio_jobs, emit, ffmpeg_ok, has_modules, run_steps, run_tool, step, why
    from zero_fx2 import add_step as add_effect_step, pack_current as effects_current
    from zero_paths import PUBLIC
    from zero_scripts2 import script_chain, script_index
    from zero_sound_health import clean_temporaries
    try:
        origin = products.destination_origin(PUBLIC, initializing=initializing)
    except (OSError, ValueError) as error:
        emit("error", message=str(error))
        return 1
    leftovers = []
    actual = products.inventory(PUBLIC, leftovers=leftovers)
    receipt = PUBLIC / ".prepare-swbf2.json"
    state = products.Receipt(receipt, GAME)
    rebuilding_old = origin == "early"
    if origin in ("early", "empty"):
        state.save(swbf2Version=PREPARED_VERSION, complete=False)
    if rebuilding_old:
        (PUBLIC / ".prepare.json").unlink()
    for path in leftovers:
        if path.is_dir():
            path.rmdir()
        else:
            path.unlink()
    products.clear_workspaces(PUBLIC)
    clean_temporaries(PUBLIC / "audio/enhanced")
    version = state.data.get("swbf2Version")
    if rebuilding_old or state.data.get("family") != "swbf2" or type(version) is not int or not 0 < version <= PREPARED_VERSION:
        state.data["done"] = []
    elif version != PREPARED_VERSION:
        state.data["done"] = [key for key in state.data["done"] if not key.startswith("map:")]
    prior = state.data.get("outputs")
    if not isinstance(prior, dict):
        prior = {}
    previous = state.data.pop("previousOutputs", None)
    if isinstance(previous, dict):
        prior = {**previous, **prior}
    outputs = {}
    errors = []
    shared_current = True
    with tempfile.TemporaryDirectory() as directory:
        sides = {}
        sounds = {}
        inputs = {}
        steps = []
        chains = {}
        effect_records = {}
        index = None
        try:
            for map_id in OPENING_MISSIONS:
                manifest = opening_manifest(map_id)
                key = f"map:{map_id}"
                if index is None:
                    index = script_index((GAME / "mission.lvl").read_bytes())
                chain = script_chain(map_id, manifest, index)
                chains[key] = (chain, manifest["script"])
                if shared_current and state.done(key):
                    if not chain.complete(PUBLIC, manifest["script"]):
                        shared_current = False
                    elif not map_products_current(PUBLIC / "maps" / map_id):
                        state.data["done"].remove(key)
                path = Path(directory) / f"{map_id}.json"
                encoded = json.dumps(manifest, sort_keys=True).encode("utf-8")
                path.write_bytes(encoded)
                effect_records[f"effects:{map_id}"] = add_effect_step(
                    steps, state, PUBLIC, map_id, manifest["script"],
                    lambda m=map_id, p=path: run_core("zero_fx2.py", m, str(p)),
                )
                inputs[map_id] = hashlib.sha256(encoded).hexdigest()
                assets = mission_assets(manifest, map_id)
                for bank in assets["sound"]:
                    sounds.setdefault(products.component(bank), []).append(key)
                for side, names in assets["sides"].items():
                    sides.setdefault(products.component(side), []).extend(names)
                steps.append(step(key, "core", map_id, 8, True,
                                  lambda m=map_id, p=path: run_core("extract.py", m, "--only", "map", "--manifest", p)))
        except (ValueError, OSError, RuntimeError) as error:
            key = f"map:{map_id}"
            state.save(complete=False, failed=[key], failures=[{"id": key, "error": str(error)}])
            emit("error", message=str(error))
            return 1
        for side, names in sides.items():
            args_for_side = [v for name in dict.fromkeys(names) for v in ("--unit", name)]
            steps.append(step(f"side:{side}", "core", side, 3, True,
                              lambda s=side, flags=args_for_side: run_core("extract.py", "--only", "side", "--side", s, *flags)))
        def common_and_first_person():
            result = run_core("extract.py", "--only", "common")
            if not result:
                return result
            flags = []
            for side, names in sides.items():
                flags.extend(("--side", side))
                for name in dict.fromkeys(names):
                    flags.extend(("--unit", name))
            with tempfile.TemporaryDirectory(dir=work) as scratch:
                result = run_tool("zero_fp2.py", "--game", GAME, "--out", scratch, *flags,
                                  env=dict(os.environ, BFE_CONTENT_DIR=scratch))
                if result:
                    (Path(scratch) / "firstperson").replace(work / "firstperson")
                return result

        steps.append(step("common", "core", "Shared animations and text", 3, True, common_and_first_person))
        available = has_modules("numpy", "scipy", "soundfile", "soxr") and ffmpeg_ok()
        for bank in sounds:
            key = f"audio:{bank}"
            if state.done(key) and not (PUBLIC / "audio" / "enhanced" / bank / "manifest.json").is_file():
                state.data["done"].remove(key)
            def enhance(b=bank):
                if any(not state.done(world) for world in sounds[b]):
                    return False
                return run_tool("enhance_audio.py", b, "--jobs", audio_jobs(args.jobs), "--stream-session", directory)
            steps.append(step(key, "extra", f"Enhancing sound: {bank}", 6, available, enhance,
                              after=("map:",), cores=audio_jobs(args.jobs)))
        emit("plan", total=sum(s[3] for s in steps), steps=[
            dict(key=k, phase=p, label=l, weight=w, available=a) for k,p,l,w,a,*_ in steps])
        keys = {s[0] for s in steps}
        for key in actual.keys() - keys - {"*"}:
            actual.setdefault("*", []).extend(actual[key])
        if state.data.get("missionInputs") != inputs:
            state.data["done"] = []
            shared_current = False
        shared = actual.get("*", [])
        shared_current = (shared_current and products.matches(prior.get("*"), shared)
                          and products.shared_present(PUBLIC, shared, sounds))
        plan = [s[0] for s in steps if s[1] != "extra" or not (args.core_only or args.no_enhanced_sound)]
        done = set(state.data["done"]) & keys
        pending, skipped = [], []
        for item in steps:
            key, phase, label, weight, ready = item[:5]
            rows = actual.get(key, [])
            if phase == "core":
                cached = (products.matches(prior.get(key), rows)
                          and products.core_present(key, rows, effect_records.get(key))
                          and (key != "common" or products.first_person_complete(PUBLIC, rows))
                          and (key not in chains or shared_current))
            else:
                cached = True  # Enhanced Sound has its own manifest and verification.
            if key in done and cached:
                outputs[key] = rows
                emit("step", key=key, status="done", cached=True, phase=phase, label=label, weight=weight)
            elif key not in plan:
                emit("step", key=key, status="off", phase=phase, label=label, weight=weight)
            elif not ready:
                skipped.append(key)
                emit("step", key=key, status="unavailable", phase=phase, label=label, weight=weight)
            else:
                pending.append(item)

        state.data["done"] = [s[0] for s in steps if s[0] in outputs]
        if shared_current:
            outputs["*"] = sorted(shared)
        state.data.update(swbf2Version=PREPARED_VERSION, missionInputs=inputs, plan=plan, outputs=outputs)
        if pending:
            state.data["previousOutputs"] = {k: v for k, v in prior.items() if k not in outputs}

        def finish(item, result, started):
            key, phase, label, weight = item[:4]
            if result:
                if phase == "extra":
                    outputs[key] = []
                state.mark(key)
            else:
                errors.append({"id": key, "label": label, "error": why(result)})
            emit("step", key=key, status="done" if result else "failed", phase=phase, label=label, weight=weight)

        core = [s for s in pending if s[1] == "core"]
        if pending:
            state.save(complete=False)
        if core:
            with products.workspace(PUBLIC) as name:
                work = Path(name)
                def run_core(tool, *arguments):
                    return run_tool(tool, *arguments, env=dict(os.environ, BFE_CONTENT_DIR=str(work)))
                completed = {}
                run_steps(core, args.jobs, lambda item, result, started: completed.update({item[0]: result}), workers=2)
                written = products.inventory(work, verify_links=False)
                maps = [s for s in core if s[0].startswith("map:")]
                for item in maps:
                    key = item[0]
                    map_dir = work / "maps" / key[4:]
                    if completed[key] and (map_dir / "map.json").is_file():
                        chain, mission = chains[key]
                        if (not (map_dir / "geom.bin").is_file() or not map_products_current(map_dir)
                                or not chain.complete(work, mission)):
                            completed[key] = Ran(False, [f"Missing geometry, vehicle pack, navigation or script chain for {key}."])
                if not shared_current and all(completed[s[0]] for s in maps):
                    try:
                        if not products.shared_present(work, written.get("*", []), sounds):
                            raise ValueError("The extraction produced an incomplete sound index, bank or stream.")
                        outputs["*"] = products.publish(PUBLIC, work, "*", written.get("*", []), shared, prior.get("*"))
                    except (OSError, ValueError) as error:
                        for item in maps:
                            completed[item[0]] = Ran(False, [str(error)])
                for item in core:
                    key = item[0]
                    result = completed[key]
                    record = effect_records.get(key)
                    if result and record and not effects_current((work / record).parent):
                        result = Ran(False, [f"Incomplete effect pack for {key}."])
                    if result:
                        try:
                            outputs[key] = products.publish(PUBLIC, work, key, written.get(key, []),
                                                           actual.get(key, []), prior.get(key), record=record)
                        except (OSError, ValueError) as error:
                            result = Ran(False, [str(error)])
                    finish(item, result, None)
        run_steps([s for s in pending if s[1] == "extra"], args.jobs, finish, workers=2)
    failed = [e["id"] for e in errors]
    missing = sum(not state.done(f"audio:{bank}") for bank in sounds)
    state.data["done"].sort()
    if not errors and not skipped:
        state.data.pop("previousOutputs", None)
    state.save(complete=not (errors or skipped or args.core_only), failed=failed, failures=errors, skipped=skipped,
               extras={"enhancedSound": missing == 0, "soundSteps": missing}, addOnFailures=[])
    if not args.core_only:
        from prepare import maintain_sound
        maintain_sound(args, locked=True)
    emit("finished", failed=failed, failures=errors, addOnFailures=[])
    return int(bool(errors))
