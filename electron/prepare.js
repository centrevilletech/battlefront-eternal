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
// failed or was skipped for a missing tool (tools/prepare.py's "complete").
// mapImportVersion must match MAP_IMPORT_VERSION in tools/prepare.py (and
// muzzleVersion and commonVersion its side and common versions). addOns
// are the add-ons installed now (discover.js: installedAddOns); a run that
// recorded others, or none before add-ons were read (Jabba's Palace missed),
// prepares again, and only what they change runs.
export function preparedContentCurrent(state) {
  return !!state && state.version === CONTENT_VERSION.state
    && state.mapImportVersion === CONTENT_VERSION.maps && state.muzzleVersion === CONTENT_VERSION.sides
    && state.commonVersion === CONTENT_VERSION.common
    && state.complete === true && Array.isArray(state.failed) && state.failed.length === 0
    && !(state.skipped?.length);
}

export function preparedFor(state, game, addOns = []) {
  return preparedContentCurrent(state)
    && typeof state.game === "string" && path.resolve(state.game) === path.resolve(game, "GameData", "Data", "_LVL_PC")
    && (Array.isArray(state.addOns) ? state.addOns : []).join("\n") === [...addOns].join("\n");
}

// Folds the tool's events into one snapshot the loader can draw from, even
// after a reload.
export function createSnapshot() {
  return { status: "idle", total: 0, done: 0, known: false, steps: [], current: null, playable: false, failed: [], error: null, log: [], startedAt: null, game: null, running: 0, parallel: 1 };
}

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
  } else if (event.event === "busy") {
    snapshot.status = "busy";
    snapshot.error = event.message;
  } else if (event.event === "error") {
    snapshot.status = "error";
    snapshot.error = event.message;
  }
  // Progress counts the steps that can run; until the catalog has named the
  // maps (no map steps yet) there is no meaningful total.
  const counted = snapshot.steps.filter((step) => step.status !== "unavailable");
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
  const steps = (snapshot.steps || []).filter((step) => step.status !== "unavailable");
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

  async start({ game, content, jobs }) {
    if (this.child) return;
    this.last = { game, content, jobs };
    await fs.mkdir(content, { recursive: true });
    await fs.mkdir(path.dirname(this.logFile), { recursive: true });
    const log = createWriteStream(this.logFile, { flags: "a" });
    log.write(`\n=== ${new Date().toISOString()} prepare ${game} -> ${content}\n`);
    this.snapshot = { ...createSnapshot(), status: "running", startedAt: Date.now(), game };
    const args = ["-u", path.join(this.paths.tools, "prepare.py")];
    if (jobs) args.push("--jobs", String(jobs));
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
