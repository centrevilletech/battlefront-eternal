import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { after, test } from "node:test";
import { existsSync, readFileSync } from "node:fs";
import { CONTENT_VERSION, applyEvent, clearPreparedFor, createSnapshot, estimate, EXPECTED, expectedSeconds, planEstimate, preparedContentCurrent, preparedFor, preparedFrom, preparedSummary, Preparer, recordTiming, toolchainPaths, toolEnv } from "../electron/prepare.js";

const temp = mkdtempSync(path.join(tmpdir(), "bfe-prepare-"));
after(() => rmSync(temp, { recursive: true, force: true }));

test("events fold into a snapshot the loader can redraw from", () => {
  const snapshot = createSnapshot();
  applyEvent(snapshot, { event: "plan", total: 10, steps: [
    { key: "catalog", phase: "core", label: "Galaxy", weight: 2, available: true },
    { key: "map:nab2:rep", phase: "core", label: "Theed", weight: 4, available: true },
    { key: "textures", phase: "extra", label: "Textures", weight: 4, available: false },
  ] });
  assert.equal(snapshot.done, 0);
  assert.equal(snapshot.total, 6);
  assert.equal(snapshot.known, true);
  applyEvent(snapshot, { event: "step", key: "catalog", label: "Galaxy", status: "start", time: 1 });
  assert.equal(snapshot.current.key, "catalog");
  applyEvent(snapshot, { event: "step", key: "catalog", status: "done" });
  assert.equal(snapshot.done, 2);
  // A second plan (after the catalog) keeps what is already known.
  applyEvent(snapshot, { event: "plan", total: 10, steps: snapshot.steps.map(({ status, ...step }) => step) });
  assert.equal(snapshot.steps[0].status, "done");
  applyEvent(snapshot, { event: "step", key: "map:nab2:rep", label: "Theed", status: "start" });
  applyEvent(snapshot, { event: "step", key: "catalog", label: "Galaxy", status: "start" });
  assert.equal(snapshot.running, 2);
  applyEvent(snapshot, { event: "step", key: "catalog", status: "done" });
  assert.equal(snapshot.current.key, "map:nab2:rep");
  assert.equal(snapshot.running, 1);
  applyEvent(snapshot, { event: "step", key: "map:nab2:rep", status: "failed" });
  assert.deepEqual(snapshot.failed, ["map:nab2:rep"]);
  applyEvent(snapshot, { event: "playable" });
  assert.equal(snapshot.playable, true);
  for (let i = 0; i < 450; i++) applyEvent(snapshot, { event: "log", line: `line ${i}` });
  assert.equal(snapshot.log.length, 400);
  applyEvent(snapshot, { event: "busy", message: "held" });
  assert.equal(snapshot.status, "busy");
  applyEvent(snapshot, { event: "finished", failed: ["map:nab2:rep"] });
  assert.equal(snapshot.status, "finished-with-errors");
});

test("the estimate counts expected time, what runs side by side, and this machine's own measurements", () => {
  const snapshot = createSnapshot();
  const step = (key, weight = 1) => ({ key, phase: "core", label: key, weight, available: true });
  applyEvent(snapshot, { event: "plan", total: 0, steps: [step("catalog"), step("map:nab2"), step("map:geo1"), step("map:hot1"), step("movies"), step("textures")] });
  applyEvent(snapshot, { event: "step", key: "textures", status: "unavailable" });
  snapshot.startedAt = 1_000_000;
  const now = 1_000_000 + 30_000;
  const fresh = estimate(snapshot, { now });
  assert.equal(fresh.known, true);
  assert.equal(fresh.fraction, 0);
  assert.equal(fresh.elapsed, 30);
  // Nothing side by side yet: the maps count one after another.
  assert.equal(fresh.remaining, EXPECTED.catalog + 3 * EXPECTED.map + EXPECTED.movies);
  // Two maps running four wide, one started 20 s ago.
  applyEvent(snapshot, { event: "step", key: "catalog", status: "done", seconds: 20 });
  applyEvent(snapshot, { event: "step", key: "map:nab2", status: "start", time: (now - 20_000) / 1000, parallel: 4 });
  applyEvent(snapshot, { event: "step", key: "map:geo1", status: "start", time: now / 1000, parallel: 4 });
  const running = estimate(snapshot, { now });
  const total = EXPECTED.catalog + 3 * EXPECTED.map + EXPECTED.movies;
  assert.ok(Math.abs(running.fraction - (EXPECTED.catalog + 20) / total) < 1e-9);
  assert.ok(Math.abs(running.remaining - ((EXPECTED.map - 20) / 4 + EXPECTED.map / 4 + EXPECTED.map / 4 + EXPECTED.movies)) < 1e-9);
  // Measured times for a kind replace the table (the median); cached and failed steps don't count.
  const timings = {};
  recordTiming(timings, { key: "map:nab2", status: "done" }, 200);
  recordTiming(timings, { key: "map:geo1", status: "done" }, 100);
  recordTiming(timings, { key: "map:hot1", status: "done" }, 150);
  recordTiming(timings, { key: "map:x", status: "failed" }, 3);
  recordTiming(timings, { key: "map:y", status: "done", cached: true }, 0.1);
  assert.deepEqual(timings, { map: [200, 100, 150] });
  assert.equal(expectedSeconds({ key: "map:kas1" }, timings), 150);
  assert.equal(expectedSeconds({ key: "hud" }, timings), EXPECTED.hud);
  for (let i = 0; i < 10; i++) recordTiming(timings, { key: "hud", status: "done" }, i);
  assert.equal(timings.hud.length, 8);
  // Three steps measured at twice their expectation scale what is left.
  for (const key of ["map:nab2", "map:geo1", "map:hot1"]) applyEvent(snapshot, { event: "step", key, status: "done", seconds: EXPECTED.map * 2 });
  const calibrated = estimate(snapshot, { now });
  assert.ok(calibrated.calibration > 1.5 && calibrated.calibration < 2.1);
  assert.ok(Math.abs(calibrated.remaining - EXPECTED.movies * calibrated.calibration) < 1e-9);
  applyEvent(snapshot, { event: "step", key: "movies", status: "done", seconds: 90 });
  applyEvent(snapshot, { event: "finished", failed: [] });
  assert.deepEqual([estimate(snapshot, { now }).fraction, estimate(snapshot, { now }).remaining], [1, 0]);
});

test("a finished run counts only for the same installation and with nothing failed", () => {
  const game = "/games/Star Wars Battlefront";
  const state = { version: CONTENT_VERSION.state, mapImportVersion: CONTENT_VERSION.maps, muzzleVersion: CONTENT_VERSION.sides, commonVersion: CONTENT_VERSION.common, complete: true, failed: [], skipped: [], game: `${game}/GameData/Data/_LVL_PC` };
  assert.equal(preparedFor(state, game), true);
  for (const mapImportVersion of [1, 5, 6, 7, CONTENT_VERSION.maps + 1]) assert.equal(preparedFor({ ...state, mapImportVersion }, game), false);
  assert.equal(preparedFor({ ...state, mapImportVersion: undefined }, game), false);
  assert.equal(preparedFor({ ...state, muzzleVersion: undefined }, game), false);
  assert.equal(preparedFor({ ...state, failed: ["movies"] }, game), false);
  assert.equal(preparedFor({ ...state, complete: false }, game), false);
  assert.equal(preparedFor({ ...state, skipped: ["hud"] }, game), false);
  assert.equal(preparedFor(state, "/elsewhere"), false);
  assert.equal(preparedFor(null, game), false);
});

test("an add-on installed since, or missed before add-ons were read, prepares again", () => {
  const game = "/games/Star Wars Battlefront";
  const state = { version: CONTENT_VERSION.state, mapImportVersion: CONTENT_VERSION.maps, muzzleVersion: CONTENT_VERSION.sides, commonVersion: CONTENT_VERSION.common, complete: true, failed: [], skipped: [], game: `${game}/GameData/Data/_LVL_PC` };
  // Prepared before tools/prepare.py recorded add-ons: Jabba's Palace (AddOn/Tat3) was never read.
  assert.equal(preparedFor(state, game, ["tat3"]), false);
  assert.equal(preparedFor({ ...state, addOns: ["tat3"] }, game, ["tat3"]), true);
  assert.equal(preparedFor({ ...state, addOns: ["tat3"] }, game, ["abc", "tat3"]), false);
  assert.equal(preparedFor({ ...state, addOns: ["abc", "tat3"] }, game, ["tat3"]), false);
  // No add-ons then or now: nothing to read.
  assert.equal(preparedFor(state, game, []), true);
  assert.equal(preparedFor({ ...state, addOns: [] }, game), true);
});

test("the tools get the bundled toolchain first on PATH", () => {
  const toolchain = path.join(temp, "toolchain");
  mkdirSync(path.join(toolchain, "bin"), { recursive: true });
  mkdirSync(path.join(toolchain, "ktx", "bin"), { recursive: true });
  mkdirSync(path.join(toolchain, "ktx", "lib"), { recursive: true });
  writeFileSync(path.join(toolchain, "ktx", "bin", "ktx"), "");
  const env = toolEnv({ toolchain, game: "/g", content: "/c", env: { PATH: "/usr/bin", PYTHONHOME: "x", ELECTRON_RUN_AS_NODE: "1" }, platform: "linux" });
  assert.equal(env.PATH, [path.join(toolchain, "bin"), path.join(toolchain, "ktx", "bin"), "/usr/bin"].join(":"));
  assert.equal(env.BFE_GAME_DIR, "/g");
  assert.equal(env.BFE_CONTENT_DIR, "/c");
  assert.equal(env.KTX2_ENCODER, path.join(toolchain, "ktx", "bin", "ktx"));
  assert.match(env.LD_LIBRARY_PATH, /ktx[\\/]lib$/);
  assert.equal(env.PYTHONHOME, undefined);
  assert.equal(env.ELECTRON_RUN_AS_NODE, undefined);
});

test("falls back to the system Python without a bundled one", () => {
  const paths = toolchainPaths({ packaged: false, projectRoot: temp, platform: "linux", arch: "x64" });
  assert.equal(paths.python, "python3");
  assert.equal(paths.bundled, false);
  assert.equal(paths.tools, path.join(temp, "tools"));
  const packaged = toolchainPaths({ packaged: true, resourcesPath: "/r", projectRoot: temp, platform: "win32" });
  assert.equal(packaged.python, "python");
  assert.equal(packaged.toolchain, path.join("/r", "toolchain"));
});

test("runs a preparation script and reports its events and exit", { skip: process.platform === "win32" }, async () => {
  const tools = path.join(temp, "tools");
  mkdirSync(tools, { recursive: true });
  writeFileSync(path.join(tools, "prepare.py"), [
    "import json, os",
    "print('@@' + json.dumps({'event': 'plan', 'total': 1, 'steps': [{'key': 'catalog', 'phase': 'core', 'label': 'x', 'weight': 1, 'available': True}]}))",
    "print('working in', os.environ['BFE_CONTENT_DIR'])",
    "print('@@' + json.dumps({'event': 'step', 'key': 'catalog', 'status': 'done', 'seconds': 1.5}))",
    "print('@@' + json.dumps({'event': 'finished', 'failed': []}))",
  ].join("\n"));
  const events = [];
  const preparer = new Preparer({
    paths: { python: "python3", tools, toolchain: path.join(temp, "none") },
    logFile: path.join(temp, "logs", "prepare.log"),
    onEvent: (event) => events.push(event.event),
  });
  const finished = new Promise((resolve) => {
    const original = preparer.onEvent;
    preparer.onEvent = (event, snapshot) => {
      original(event, snapshot);
      if (event.event === "finished") setTimeout(resolve, 50);
    };
  });
  await preparer.start({ game: "/g", content: path.join(temp, "out") });
  await finished;
  assert.deepEqual(events, ["plan", "log", "step", "finished"]);
  assert.equal(preparer.snapshot.status, "finished");
  assert.match(preparer.snapshot.log[0], /working in .*out/);
  assert.deepEqual(preparer.timings, { catalog: [1.5] });
  assert.equal(preparer.estimate().fraction, 1);
});

test("a run's own statuses: the Enhanced sound off and add-on steps left out don't count toward progress", () => {
  const snapshot = createSnapshot();
  const step = (key) => ({ key, phase: "core", label: key, weight: 2, available: true });
  applyEvent(snapshot, { event: "plan", total: 0, steps: [step("catalog"), step("map:nab2"), step("map:mod1"), step("audio:nab2cw")] });
  applyEvent(snapshot, { event: "step", key: "audio:nab2cw", status: "off" });
  applyEvent(snapshot, { event: "step", key: "map:mod1", status: "left-out" });
  assert.equal(snapshot.total, 4);
  applyEvent(snapshot, { event: "step", key: "catalog", status: "done" });
  applyEvent(snapshot, { event: "step", key: "map:nab2", status: "done" });
  assert.equal(snapshot.done, 4);
  assert.deepEqual(snapshot.failed, [], "left out is not failed");
  const left = [{ addOn: "Mod", map: "mod1", name: "MOD ONE", part: "map", step: "map:mod1", reason: "no archive" }];
  applyEvent(snapshot, { event: "finished", failed: [], failures: [], addOnFailures: left });
  assert.equal(snapshot.status, "finished");
  assert.deepEqual(snapshot.addOnFailures, left);
  assert.equal(estimate(snapshot).fraction, 1);
});

test("the gate: the chosen profile complete, add-on maps left out, the game's own failures not", () => {
  const game = "/games/Star Wars Battlefront";
  const state = { version: CONTENT_VERSION.state, mapImportVersion: CONTENT_VERSION.maps, muzzleVersion: CONTENT_VERSION.sides, commonVersion: CONTENT_VERSION.common, complete: true, failed: [], skipped: [], game: `${game}/GameData/Data/_LVL_PC`, addOns: [] };
  const without = { ...state, extras: { enhancedSound: false, soundSteps: 30 } };
  assert.equal(preparedContentCurrent(without), true);
  assert.equal(preparedFor(without, game, [], { enhancedSound: false }), true);
  assert.equal(preparedFor(without, game, [], { enhancedSound: true }), false, "wanted again: prepare only it");
  assert.equal(preparedFor(without, game), true, "no wish given: as prepared");
  assert.equal(preparedFor({ ...state, addOnFailures: [{ map: "mod1" }] }, game), true);
  assert.equal(preparedFor({ ...state, failed: ["map:nab2"], failures: [{ key: "map:nab2" }] }, game), false);
  assert.equal(preparedFrom(state, game), true);
  assert.equal(preparedFrom({ ...state, complete: false, mapImportVersion: 1 }, game), true, "finished or not");
  assert.equal(preparedFrom(state, "/elsewhere"), false);
  assert.equal(preparedFrom(state, game.toUpperCase(), "win32"), true, "Windows folds case");
  assert.equal(preparedFrom(state, game.toUpperCase(), "linux"), false);
  assert.equal(preparedFrom(null, game), false);
  assert.deepEqual(preparedSummary(without), { addOnFailures: [], enhancedSound: false, soundSteps: 30, failures: [] });
  assert.deepEqual(preparedSummary(null), { addOnFailures: [], enhancedSound: true, soundSteps: 0, failures: [] });
});

test("another copy prepared into a folder clears what the last one left there, only preparation's own outputs", async () => {
  const content = path.join(temp, "clear-content");
  const game = path.join(temp, "clear-copy-a");
  const fill = (state) => {
    for (const name of ["maps/mod1", "sides/rep", "common/shell", "audio/enhanced"]) mkdirSync(path.join(content, name), { recursive: true });
    writeFileSync(path.join(content, "maps/mod1/map.json"), "{}");
    writeFileSync(path.join(content, ".prepare.json"), JSON.stringify(state));
    writeFileSync(path.join(content, "keep.txt"), "not preparation's");
    mkdirSync(path.join(content, ".locks"), { recursive: true });
  };
  fill({ game: path.join(game, "GameData", "Data", "_LVL_PC") });
  assert.equal(await clearPreparedFor(content, game), false, "the same copy keeps everything (it resumes)");
  assert.ok(existsSync(path.join(content, "maps/mod1/map.json")));
  assert.equal(await clearPreparedFor(content, path.join(temp, "clear-copy-b")), true);
  for (const name of ["maps", "sides", "common", "audio", ".prepare.json"]) assert.equal(existsSync(path.join(content, name)), false, name);
  assert.equal(readFileSync(path.join(content, "keep.txt"), "utf8"), "not preparation's");
  assert.ok(existsSync(path.join(content, ".locks")));
  // Outputs nobody recorded (no .prepare.json) can't be told apart: they go too.
  fill({});
  rmSync(path.join(content, ".prepare.json"));
  assert.equal(await clearPreparedFor(content, game), true);
  assert.equal(existsSync(path.join(content, "maps")), false);
  assert.equal(await clearPreparedFor(path.join(temp, "clear-nothing"), game), false);
});

test("before a run, the check's counts give about how long it takes and what the Enhanced sound adds", () => {
  const check = { counts: { maps: 17, sides: 11, sublevels: 30 }, workers: 6 };
  const time = planEstimate(check);
  const fixed = EXPECTED.catalog + EXPECTED.common + EXPECTED.shell + EXPECTED["catalog-update"] + EXPECTED.environment + EXPECTED.hud + EXPECTED.textures + EXPECTED.movies;
  assert.ok(Math.abs(time.sound - 30 * EXPECTED.audio) < 1e-6);
  assert.ok(Math.abs(time.total - (fixed + (11 * EXPECTED.side + 17 * EXPECTED.map) / 6 + 30 * EXPECTED.audio)) < 1e-6);
  assert.ok(Math.abs(planEstimate(check, { enhancedSound: false }).total - (time.total - time.sound)) < 1e-6);
  // This machine's own measurements count, as during the run.
  assert.ok(Math.abs(planEstimate(check, { timings: { audio: [10] } }).sound - 300) < 1e-6);
  assert.equal(planEstimate({}), null);
});

test("the check runs the tools' --check and returns its event, or says why it couldn't", { skip: process.platform === "win32" }, async () => {
  const tools = path.join(temp, "check-tools");
  mkdirSync(tools, { recursive: true });
  writeFileSync(path.join(tools, "prepare.py"), [
    "import json, os, sys",
    "assert '--check' in sys.argv",
    "print('reading', os.environ['BFE_GAME_DIR'])",
    "print('@@' + json.dumps({'event': 'check', 'ok': os.environ['BFE_GAME_DIR'] == '/good', 'problems': [], 'addOns': [], 'counts': {'maps': 1, 'sides': 2, 'sublevels': 3}, 'workers': 2}))",
    "sys.exit(0 if os.environ['BFE_GAME_DIR'] == '/good' else 1)",
  ].join("\n"));
  const preparer = new Preparer({ paths: { python: "python3", tools, toolchain: path.join(temp, "none") }, logFile: path.join(temp, "logs", "check.log") });
  const good = await preparer.check({ game: "/good", content: path.join(temp, "check-out") });
  assert.equal(good.ok, true);
  assert.deepEqual(good.counts, { maps: 1, sides: 2, sublevels: 3 });
  const bad = await preparer.check({ game: "/bad", content: path.join(temp, "check-out") });
  assert.equal(bad.ok, false);
  assert.equal(bad.unavailable, undefined, "a copy the check found wrong is not a check that couldn't run");
  assert.equal(existsSync(path.join(temp, "check-out")), false, "a check writes nothing");
  // A check that couldn't run says so (unavailable): it says nothing about the copy.
  writeFileSync(path.join(tools, "prepare.py"), "import time\ntime.sleep(30)\n");
  const slow = await preparer.check({ game: "/good", content: temp, timeoutMs: 300 });
  assert.equal(slow.ok, false);
  assert.equal(slow.unavailable, true);
  assert.match(slow.problems[0].message, /took too long/);
  const missing = new Preparer({ paths: { python: path.join(temp, "no-python"), tools, toolchain: temp }, logFile: path.join(temp, "logs", "check.log") });
  const none = await missing.check({ game: "/good", content: temp });
  assert.equal(none.ok, false);
  assert.equal(none.unavailable, true);
  assert.match(none.problems[0].message, /couldn't check/);
});

test("a run without the Enhanced sound asks the tools to leave it out", { skip: process.platform === "win32" }, async () => {
  const tools = path.join(temp, "flag-tools");
  mkdirSync(tools, { recursive: true });
  writeFileSync(path.join(tools, "prepare.py"), [
    "import json, sys",
    "print('@@' + json.dumps({'event': 'finished', 'failed': [], 'args': sys.argv[1:]}))",
  ].join("\n"));
  for (const [enhancedSound, flagged] of [[false, true], [true, false]]) {
    const events = [];
    const preparer = new Preparer({ paths: { python: "python3", tools, toolchain: path.join(temp, "none") }, logFile: path.join(temp, "logs", "flag.log"), onEvent: (event) => events.push(event) });
    const finished = new Promise((resolve) => { preparer.onEvent = (event) => { events.push(event); if (event.event === "finished") resolve(); }; });
    await preparer.start({ game: "/g", content: path.join(temp, "flag-out"), jobs: 2, enhancedSound });
    await finished;
    assert.equal(events.find((event) => event.event === "finished").args.includes("--no-enhanced-sound"), flagged);
    assert.equal(preparer.snapshot.enhancedSound, enhancedSound);
    assert.equal(preparer.last.enhancedSound, enhancedSound, "a busy retry keeps the choice");
  }
});
