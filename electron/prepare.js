// Runs tools/prepare.py with the bundled toolchain and reports its progress.
// One run at a time, at normal priority with every core (nothing else runs
// while the loader is up); it is stopped, whole process tree, when the app
// quits, and the next launch resumes it. The loader's time estimate comes
// from measured step durations (estimate, below).
import { execFile, spawn } from "node:child_process";
import { createWriteStream, existsSync, readFileSync, promises as fs } from "node:fs";
import path from "node:path";

// The toolchain (build/fetch-toolchain.mjs) and the tools, packaged as
// resources, or in a checkout toolchain/<os>-<arch> and tools/.
export function toolchainPaths({ packaged, resourcesPath, projectRoot, platform = process.platform, arch = process.arch }) {
  const os = { linux: "linux", darwin: "mac", win32: "win" }[platform];
  const toolchain = packaged ? path.join(resourcesPath, "toolchain") : path.join(projectRoot, "toolchain", `${os}-${arch}`);
  const tools = packaged ? path.join(resourcesPath, "tools") : path.join(projectRoot, "tools");
  const windows = platform === "win32";
  const bundled = windows ? path.join(toolchain, "python", "python.exe") : path.join(toolchain, "python", "bin", "python3");
  const python = existsSync(bundled) ? bundled : windows ? "python" : "python3";
  return { toolchain, tools, python, bundled: python === bundled };
}

export function toolEnv({ toolchain, game, content, env = process.env, platform = process.platform }) {
  const sep = platform === "win32" ? ";" : ":";
  const bin = [path.join(toolchain, "bin"), path.join(toolchain, "ktx", "bin")].filter(existsSync);
  const next = {
    ...env,
    BFE_GAME_DIR: game,
    BFE_CONTENT_DIR: content,
    PATH: [...bin, env.PATH || env.Path || ""].join(sep),
    PYTHONUNBUFFERED: "1",
    PYTHONDONTWRITEBYTECODE: "1",
    PYTHONIOENCODING: "utf-8",
  };
  delete next.PYTHONHOME;
  delete next.PYTHONPATH;
  delete next.ELECTRON_RUN_AS_NODE;
  const ktx = path.join(toolchain, "ktx", "bin", platform === "win32" ? "ktx.exe" : "ktx");
  if (existsSync(ktx)) next.KTX2_ENCODER = ktx;
  const lib = path.join(toolchain, "ktx", "lib");
  if (existsSync(lib) && platform === "linux") next.LD_LIBRARY_PATH = [lib, env.LD_LIBRARY_PATH].filter(Boolean).join(":");
  if (existsSync(lib) && platform === "darwin") next.DYLD_LIBRARY_PATH = [lib, env.DYLD_LIBRARY_PATH].filter(Boolean).join(":");
  return next;
}

export async function readPrepareState(content) {
  try {
    return JSON.parse(await fs.readFile(path.join(content, ".prepare.json"), "utf8"));
  } catch {
    return null;
  }
}

// electron-builder keeps extraResources outside app.asar. Development uses
// the checkout copy; the packaged client shares the Python tools' one copy.
const checkoutVersion = new URL('../tools/content-version.json', import.meta.url);
const versionFile = existsSync(checkoutVersion) ? checkoutVersion : path.join(process.resourcesPath, 'tools/content-version.json');
export const CONTENT_VERSION = Object.freeze(JSON.parse(readFileSync(versionFile, 'utf8')));

// Whether a full run left the game complete for this installation: nothing
// of the game's own failed or was skipped for a missing tool
// (tools/prepare.py's "complete", for the profile the player chose: without
// the Enhanced sound, extras.enhancedSound false, it is complete too). An
// add-on map that couldn't be prepared is left out (addOnFailures), not
// failed, so a broken mod never keeps the game from starting.
// mapImportVersion must match MAP_IMPORT_VERSION in tools/prepare.py (and
// muzzleVersion and commonVersion its side and common versions).
export function preparedContentCurrent(state) {
  return !!state && state.version === CONTENT_VERSION.state
    && state.mapImportVersion === CONTENT_VERSION.maps && state.muzzleVersion === CONTENT_VERSION.sides
    && state.commonVersion === CONTENT_VERSION.common
    && state.complete === true && Array.isArray(state.failed) && state.failed.length === 0
    && !(state.skipped?.length);
}

// The installation's _LVL_PC, as .prepare.json records the game.
const gameData = (game) => path.resolve(game, "GameData", "Data", "_LVL_PC");

// Whether this folder was prepared from that installation, finished or not.
// Compared as the file system compares names: Windows and macOS fold case,
// so C:\Games and c:\games are one copy (and its game data isn't cleared).
export function preparedFrom(state, game, platform = process.platform) {
  if (typeof state?.game !== "string") return false;
  const [recorded, wanted] = [path.resolve(state.game), gameData(game)];
  return platform === "linux" ? recorded === wanted : recorded.toLowerCase() === wanted.toLowerCase();
}

// Complete for this installation with the add-ons installed now
// (discover.js: installedAddOns); a run that recorded others, or none before
// add-ons were read (Jabba's Palace missed), prepares again, and only what
// they change runs. wanted: the player's choices; Enhanced sound wanted and
// left out before prepares only it.
export function preparedFor(state, game, addOns = [], wanted = {}) {
  return preparedContentCurrent(state) && preparedFrom(state, game)
    && (Array.isArray(state.addOns) ? state.addOns : []).join("\n") === [...addOns].join("\n")
    && !(wanted.enhancedSound === true && state.extras?.enhancedSound === false);
}

// What a finished folder lacks, for the loader: the add-on maps (or parts of
// them) left out, and whether the Enhanced sound was left out by choice.
export function preparedSummary(state) {
  return {
    addOnFailures: Array.isArray(state?.addOnFailures) ? state.addOnFailures : [],
    enhancedSound: state?.extras?.enhancedSound !== false,
    soundSteps: Number.isInteger(state?.extras?.soundSteps) ? state.extras.soundSteps : 0,
    failures: Array.isArray(state?.failures) ? state.failures : [],
  };
}

// What preparation writes into a game data folder (tools/prepare.py): all
// of it comes from the installation it was prepared from.
export const PREPARED_OUTPUTS = ["maps", "sides", "common", "audio", ".prepare.json"];

// Before another installation is prepared into the app's own folder, what
// the last one left goes (a modded copy's maps, say), so nothing of it
// stays. Only the outputs above, and only when the folder was prepared from
// another copy or from one it can't tell. Never for a folder the player
// picked: the caller decides that. Whether anything was removed.
export async function clearPreparedFor(content, game) {
  const state = await readPrepareState(content);
  if (preparedFrom(state, game)) return false;
  let removed = false;
  for (const name of PREPARED_OUTPUTS) {
    const target = path.join(content, name);
    if (!existsSync(target)) continue;
    await fs.rm(target, { recursive: true, force: true });
    removed = true;
  }
  return removed;
}

// Folds the tool's events into one snapshot the loader can draw from, even
// after a reload.
export function createSnapshot() {
  return { status: "idle", total: 0, done: 0, known: false, steps: [], current: null, playable: false, failed: [], failures: [], addOnFailures: [], error: null, log: [], startedAt: null, game: null, running: 0, parallel: 1, enhancedSound: true };
}

// Steps that won't run: a tool is missing (unavailable), the player left the
// Enhanced sound out (off), or the step is a left-out add-on map's.
export const IDLE = new Set(["unavailable", "off", "left-out"]);

export function applyEvent(snapshot, event) {
  if (event.event === "log") {
    snapshot.log.push(event.line);
    if (snapshot.log.length > 400) snapshot.log.splice(0, snapshot.log.length - 400);
    return snapshot;
  }
  if (event.event === "plan") {
    const previous = new Map(snapshot.steps.map((step) => [step.key, step]));
    snapshot.steps = event.steps.map((step) => {
      const known = previous.get(step.key);
      return { ...step, status: known?.status || (step.available ? "pending" : "unavailable"), since: known?.since ?? null, seconds: known?.seconds ?? null, cached: known?.cached ?? false };
    });
  } else if (event.event === "step") {
    const step = snapshot.steps.find((entry) => entry.key === event.key);
    if (step) {
      step.status = event.status === "start" ? "running" : event.status;
      if (event.status === "start") step.since = event.time ? event.time * 1000 : Date.now();
      if (typeof event.seconds === "number") step.seconds = event.seconds;
      if (event.cached) step.cached = true;
    }
    if (event.status === "start") snapshot.current = { key: event.key, label: event.label, phase: event.phase, since: event.time };
    if (event.status === "start" && event.parallel) snapshot.parallel = event.parallel;
    if (event.status === "failed") snapshot.failed.push(event.key);
    // Maps run side by side: when the one shown ends, show another still going.
    if (event.status !== "start" && snapshot.current?.key === event.key) {
      const other = snapshot.steps.find((entry) => entry.status === "running");
      snapshot.current = other ? { key: other.key, label: other.label, phase: other.phase } : null;
    }
    snapshot.running = snapshot.steps.filter((entry) => entry.status === "running").length;
  } else if (event.event === "playable") {
    snapshot.playable = true;
  } else if (event.event === "finished") {
    snapshot.status = event.failed?.length ? "finished-with-errors" : "finished";
    snapshot.current = null;
    snapshot.failures = Array.isArray(event.failures) ? event.failures : [];
    snapshot.addOnFailures = Array.isArray(event.addOnFailures) ? event.addOnFailures : [];
  } else if (event.event === "busy") {
    snapshot.status = "busy";
    snapshot.error = event.message;
  } else if (event.event === "error") {
    snapshot.status = "error";
    snapshot.error = event.message;
  }
  // Progress counts the steps that can run; until the catalog has named the
  // maps (no map steps yet) there is no meaningful total.
  const counted = snapshot.steps.filter((step) => !IDLE.has(step.status));
  snapshot.known = snapshot.steps.some((step) => step.key.startsWith("map:"));
  snapshot.total = counted.reduce((sum, step) => sum + step.weight, 0);
  snapshot.done = counted.reduce((sum, step) => sum + (["done", "failed"].includes(step.status) ? step.weight : 0), 0);
  return snapshot;
}

// ---- The time estimate ----

// Seconds a step of each kind takes, measured on an 8-core laptop on
// 2026-10-10 (logs/prepare.log): a map's extraction 105 s on average (48 to
// 135) with four side by side, a sub-level's sound enhancement 40 s, the
// textures 100 s, the movies 86 s, the HUD 23 s, the fountains 11 s. The
// armies and the common data as steps of their own are guesses until this
// machine measures them (timings, kept by the app across runs).
export const EXPECTED = { catalog: 20, "catalog-update": 5, side: 60, common: 90, shell: 30, map: 105, environment: 11, hud: 23, audio: 40, textures: 100, movies: 86 };
export const kindOf = (key) => String(key || "").split(":")[0];
const finished = (step) => step.status === "done" || step.status === "failed";
const batched = (key) => key.startsWith("map:") || key.startsWith("side:");

// A step's expected seconds: the median of what this machine measured for
// its kind (timings: { kind: [seconds...] }), else the table.
export function expectedSeconds(step, timings = null) {
  const measured = timings?.[kindOf(step.key)];
  if (Array.isArray(measured) && measured.length) {
    const sorted = measured.slice().sort((a, b) => a - b);
    return sorted[Math.floor(sorted.length / 2)];
  }
  return EXPECTED[kindOf(step.key)] ?? Math.max(10, (step.weight || 1) * 10);
}

// Records a finished step's time for the next estimate: the last eight of
// its kind. Failed and cached steps don't count (they stop early).
export function recordTiming(timings, step, seconds) {
  if (!(seconds > 0) || step.status !== "done" || step.cached) return timings;
  const list = (timings[kindOf(step.key)] ||= []);
  list.push(seconds);
  if (list.length > 8) list.splice(0, list.length - 8);
  return timings;
}

// { known, fraction, elapsed, remaining, calibration }: progress by
// expected time rather than by step count (a map counts for its two
// minutes, the movies for theirs); elapsed this session (s); remaining (s)
// from the steps still to run, the maps and sides divided by how many run
// side by side (the tool's `parallel`), and scaled by how this session's
// finished steps compared with their expected times (the calibration,
// 0.5 to 3) once three have finished.
export function estimate(snapshot, { timings = null, now = Date.now() } = {}) {
  const steps = (snapshot.steps || []).filter((step) => !IDLE.has(step.status));
  let total = 0, done = 0, remaining = 0, measured = 0, expectedOfMeasured = 0, samples = 0;
  for (const step of steps) {
    const e = expectedSeconds(step, timings);
    const w = batched(step.key) ? Math.max(1, snapshot.parallel || 1) : 1;
    total += e;
    if (finished(step)) {
      done += e;
      if (step.seconds > 0 && !step.cached && step.status === "done") {
        measured += step.seconds;
        expectedOfMeasured += e;
        samples += 1;
      }
    } else if (step.status === "running") {
      const part = Math.min(e * 0.95, step.since ? Math.max(0, (now - step.since) / 1000) : 0);
      done += part;
      remaining += (e - part) / w;
    } else {
      remaining += e / w;
    }
  }
  const calibration = samples >= 3 && expectedOfMeasured > 0 ? Math.min(3, Math.max(0.5, measured / expectedOfMeasured)) : 1;
  const over = snapshot.status === "finished" || snapshot.status === "finished-with-errors";
  return {
    known: !!snapshot.known,
    fraction: over ? 1 : total ? Math.min(0.999, done / total) : 0,
    elapsed: snapshot.startedAt ? Math.max(0, (now - snapshot.startedAt) / 1000) : 0,
    remaining: over ? 0 : remaining * calibration,
    calibration,
  };
}

// Before a run, from the installation's check (tools/prepare.py --check:
// counts, workers): about how long preparing takes ({ total, sound } in
// seconds, sound being what the Enhanced sound adds), by the estimate above,
// so it matches what the progress screen shows when the run starts.
export function planEstimate({ counts, workers } = {}, { timings = null, enhancedSound = true } = {}) {
  if (!counts) return null;
  const step = (key) => ({ key, weight: 1, status: "pending" });
  const steps = ["catalog", "common", "shell", "catalog-update", "environment", "hud", "textures", "movies"].map(step);
  for (let i = 0; i < counts.sides; i++) steps.push(step(`side:${i}`));
  for (let i = 0; i < counts.maps; i++) steps.push(step(`map:${i}`));
  const sound = Array.from({ length: counts.sublevels || 0 }, (_, i) => step(`audio:${i}`));
  const parallel = Math.max(1, workers || 1);
  const seconds = (list) => estimate({ steps: list, known: true, parallel, status: "running" }, { timings }).remaining;
  const withSound = seconds([...steps, ...sound]);
  const without = seconds(steps);
  return { total: enhancedSound ? withSound : without, sound: withSound - without };
}

export class Preparer {
  constructor({ paths, logFile, onEvent, timings = null, onTiming = null }) {
    this.paths = paths;
    this.logFile = logFile;
    this.onEvent = onEvent;
    this.timings = timings || {};
    this.onTiming = onTiming;
    this.child = null;
    this.snapshot = createSnapshot();
  }

  get running() {
    return !!this.child;
  }

  estimate(now = Date.now()) {
    return estimate(this.snapshot, { timings: this.timings, now });
  }

  emit(event) {
    applyEvent(this.snapshot, event);
    if (event.event === "step" && typeof event.seconds === "number") {
      const step = this.snapshot.steps.find((entry) => entry.key === event.key) || { key: event.key, status: event.status, cached: !!event.cached };
      recordTiming(this.timings, step, event.seconds);
      this.onTiming?.(this.timings);
    }
    this.onEvent?.(event, this.snapshot);
  }

  // The installation's check (tools/prepare.py --check), before anything is
  // converted: { ok, problems, addOns, counts, workers }, or { ok: false,
  // problems } saying the check itself couldn't run. Writes nothing.
  check({ game, content, jobs, timeoutMs = 120000 }) {
    return new Promise((resolve) => {
      const args = ["-u", path.join(this.paths.tools, "prepare.py"), "--check"];
      if (jobs) args.push("--jobs", String(jobs));
      let output = "", settled = false, timer = null;
      const done = (result) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        resolve(result);
      };
      const failed = (message) => done({ ok: false, problems: [{ code: "check", path: null, message }], addOns: [] });
      let child;
      try {
        child = execFile(this.paths.python, args, { cwd: this.paths.tools, env: toolEnv({ toolchain: this.paths.toolchain, game, content }), windowsHide: true, maxBuffer: 16 << 20 },
          (error, stdout) => {
            output = stdout || output;
            const line = String(output).split(/\r?\n/).find((entry) => entry.startsWith("@@") && entry.includes('"check"'));
            try {
              const event = line && JSON.parse(line.slice(2));
              if (event?.event === "check") return done(event);
            } catch {
              // reported below
            }
            failed(`The preparation tools couldn't check this copy${error?.message ? ` (${error.message.split("\n")[0]})` : ""}.`);
          });
      } catch (error) {
        return failed(`The preparation tools couldn't start (${error.message}).`);
      }
      timer = setTimeout(() => {
        child.kill();
        failed("Checking this copy took too long. Is it on a drive that's asleep or disconnected?");
      }, timeoutMs);
    });
  }

  async start({ game, content, jobs, enhancedSound = true }) {
    if (this.child) return;
    this.last = { game, content, jobs, enhancedSound };
    await fs.mkdir(content, { recursive: true });
    await fs.mkdir(path.dirname(this.logFile), { recursive: true });
    const log = createWriteStream(this.logFile, { flags: "a" });
    log.write(`\n=== ${new Date().toISOString()} prepare ${game} -> ${content}${enhancedSound ? "" : " (no Enhanced sound)"}\n`);
    this.snapshot = { ...createSnapshot(), status: "running", startedAt: Date.now(), game, enhancedSound };
    const args = ["-u", path.join(this.paths.tools, "prepare.py")];
    if (jobs) args.push("--jobs", String(jobs));
    if (!enhancedSound) args.push("--no-enhanced-sound");
    const child = spawn(this.paths.python, args, {
      cwd: this.paths.tools,
      env: toolEnv({ toolchain: this.paths.toolchain, game, content }),
      detached: process.platform !== "win32",
      windowsHide: true,
      stdio: ["ignore", "pipe", "pipe"],
    });
    this.child = child;
    const lines = (stream) => {
      let buffer = "";
      stream.setEncoding("utf8");
      stream.on("data", (chunk) => {
        log.write(chunk);
        buffer += chunk;
        const parts = buffer.split(/\r?\n/);
        buffer = parts.pop();
        for (const line of parts) this.line(line);
      });
    };
    lines(child.stdout);
    lines(child.stderr);
    child.on("error", (error) => this.emit({ event: "error", message: `Could not start the preparation tools: ${error.message}` }));
    child.on("exit", (code, signal) => {
      log.end(`=== exit ${code ?? signal}\n`);
      this.child = null;
      if (this.snapshot.status === "running") {
        this.emit(signal || code === null
          ? { event: "error", message: "Preparation stopped. It will resume next time." }
          : code === 0 ? { event: "finished", failed: [] } : { event: "error", message: `Preparation failed (exit ${code}). See the log.` });
      }
    });
  }

  line(line) {
    if (line.startsWith("@@")) {
      try {
        return this.emit(JSON.parse(line.slice(2)));
      } catch {
        // fall through: show it as a log line
      }
    }
    if (line.trim()) this.emit({ event: "log", line: line.slice(0, 500) });
  }

  stop() {
    const child = this.child;
    if (!child) return;
    if (process.platform === "win32") execFile("taskkill", ["/pid", String(child.pid), "/T", "/F"], () => {});
    else {
      try {
        process.kill(-child.pid, "SIGTERM");
      } catch {
        child.kill("SIGTERM");
      }
    }
  }
}
