#!/usr/bin/env node
// The dedicated server's updater (docs/server-updates.md). It follows the
// signed release channel the players' clients follow, so a game server runs
// the engine and server adapter of the newest release, on game data that
// release's own tools prepared: what every client of that release has. A
// systemd timer (bfeserver-update.timer) runs it every five minutes:
//
// 1. Check the channel. A release newer than the one running, and not one
//    that failed here, is installed into <root>/versions/<version>: the host
//    runtime (engine, server adapter, this updater), the tools and the
//    toolchain only, checked file by file as a client's update is.
// 2. Prepare its game data from the owned game in <retail>, with its own
//    tools and toolchain (tools/prepare.py --core-only, into an empty
//    folder, as the server's user, at the updater's idle priority), unless a
//    release with the same tools did already: <root>/data/<key>. Never
//    refreshed in place, so nothing an earlier tool wrote survives.
// 3. Check them: the engine's network contract loads, and every battle of
//    the server's rotation fingerprints (src/runtime/content-hash.js).
// 4. Once nobody is playing on a server, stop it, point its engine, server
//    and game-data links at the new release, start it, and wait for health:
//    running, multiplayer ready, the release's contract, ticks advancing. A
//    release that fails is rolled back and set aside until a newer one.
//    While players are on, the next run looks again. Each server of the
//    machine (`instances`) moves on its own, once it is empty.
// 5. With rebootWhenRequired, restart the machine when its updates ask for it
//    (/var/run/reboot-required), once every server is empty.
import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fsSync, { promises as fs } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { FEED, fetchRelease, fetchTree } from "./feed.js";
import { installRelease } from "./install.js";
import { UPDATE_KEYS } from "./keys.js";
import { INSTALL_RECORD, installedVersions, platformKey, versionsDir } from "./state.js";
import { isNewer, parseVersion } from "./version.js";

// What a server takes of a release, and what its game data depends on.
export const SERVER_PARTS = ["resources/host-runtime/", "resources/tools/", "resources/toolchain/"];
export const DATA_INPUTS = ["resources/tools/", "resources/toolchain/python/"];
// The links a server runs through, to their places in a version.
export const RUNTIME = { engine: "resources/host-runtime/engine", server: "resources/host-runtime/server", update: "resources/host-runtime/update" };
// What a server install counts as its executable (state.js lists a version
// only while it is there), the record in prepared data, and a version's note
// of which data it uses.
export const SERVER_EXECUTABLE = "resources/host-runtime/server/src/main.js";
export const DATA_RECORD = ".bfe-data.json", VERSION_DATA = ".bfe-game-data.json";
const STATE_FILE = "update-state.json", LOCK = "update.lock", MAX_PREPARE_ATTEMPTS = 3;

export const DEFAULTS = Object.freeze({
  channel: "development-preview",
  feed: FEED,
  root: "/opt/bfeserver",
  stateDir: "/var/lib/bfeserver",
  service: "bfeserver",
  user: "bfeserver",
  retail: "/opt/bfeserver/retail",
  jobs: 2,
  health: "http://127.0.0.1:18450/healthz",
  info: "http://127.0.0.1:18451/info",
  healthTimeoutMs: 180000,
  keep: 2,
  hold: false,
  rebootWhenRequired: false,
  rebootFlag: "/var/run/reboot-required",
  // A server unit the updater writes takes this much memory at most.
  memoryMax: "1500M",
});

// The servers this machine runs (config.instances), each with what it is
// reached by: its systemd service, the folder its engine, server and
// game-data links are in, its own state folder (config.json, identity) and
// its health and info addresses. Without instances, one server: the
// configuration's own service, root, stateDir, health and info ("main").
export function instancesOf(config) {
  const list = Array.isArray(config.instances) && config.instances.length ? config.instances : [{ name: "main" }];
  const names = new Set();
  return list.map((own) => {
    const name = own.name ?? "main";
    if (!/^[a-z0-9-]{1,32}$/.test(name) || names.has(name)) throw Error(`Invalid or repeated instance name ${JSON.stringify(name)}`);
    names.add(name);
    const main = name === "main";
    const local = (port, route) => (Number.isInteger(port) ? `http://127.0.0.1:${port}/${route}` : null);
    const instance = {
      name,
      service: own.service ?? (main ? config.service : `bfeserver-${name}`),
      links: own.links ?? (main ? config.root : path.join(config.root, "instances", name)),
      stateDir: own.stateDir ?? (main ? config.stateDir : `/var/lib/bfeserver-${name}`),
      healthPort: own.healthPort ?? null,
      health: own.health ?? local(own.healthPort, "healthz") ?? (main ? config.health : null),
      info: own.info ?? local(own.gamePort, "info") ?? (main ? config.info : null),
      memoryMax: own.memoryMax ?? config.memoryMax,
      healthTimeoutMs: own.healthTimeoutMs ?? config.healthTimeoutMs,
      // The updater writes the unit of every server but the main one, which
      // the operator set up.
      unit: own.unit ?? !main,
    };
    if (!instance.health || !instance.info) throw Error(`Instance ${name} needs healthPort and gamePort (or health and info)`);
    return instance;
  });
}

const log = (message) => console.log(`bfeserver-update: ${message}`);
const exists = (file) => fsSync.existsSync(file);
const lexists = (file) => { try { fsSync.lstatSync(file); return true; } catch { return false; } };

// ---- Releases ----

// A client manifest cut down to what a server installs.
export function serverTree(tree) {
  const keep = (file) => SERVER_PARTS.some((part) => file.startsWith(part));
  const files = tree.files.filter((file) => keep(file.path));
  if (!files.some((file) => file.path === SERVER_EXECUTABLE)) throw Error("The release has no server runtime");
  return { ...tree, executable: SERVER_EXECUTABLE, files, links: (tree.links ?? []).filter((link) => keep(link.path)), dirs: (tree.dirs ?? []).filter(keep) };
}

// The game data a release prepares is named by its inputs: the tools and
// the Python they run on (the manifest's hashes of them). Releases with the
// same inputs share prepared data.
export function dataKey(tree) {
  const lines = tree.files.filter((file) => DATA_INPUTS.some((part) => file.path.startsWith(part))).map((file) => `${file.path} ${file.sha256}`).sort();
  return createHash("sha256").update(lines.join("\n")).digest("hex").slice(0, 32);
}

export function readUpdateState(root) {
  let state = {};
  try { state = JSON.parse(fsSync.readFileSync(path.join(root, STATE_FILE), "utf8")) ?? {}; } catch {}
  state = { bad: [], prepareFailures: {}, instances: {}, ...state };
  // A state from before instances: it described the main server.
  if (state.current || state.pending) {
    state.instances.main = { current: state.current ?? null, previous: state.previous ?? null, pending: state.pending, updatedAt: state.updatedAt };
    for (const key of ["current", "previous", "pending", "updatedAt"]) delete state[key];
  }
  return state;
}

function writeUpdateState(root, state) {
  const file = path.join(root, STATE_FILE), temp = `${file}.${process.pid}.tmp`;
  fsSync.writeFileSync(temp, JSON.stringify(state, null, 2) + "\n");
  fsSync.renameSync(temp, file);
}

// Where each of a server's links in dir points now: { engine, server,
// game-data, update } as absolute targets, null for a folder of its own (a
// manual deploy) or nothing there.
export function currentLinks(dir, names = ["engine", "server", "game-data", "update"]) {
  const out = {};
  for (const name of names) {
    const at = path.join(dir, name);
    try { out[name] = fsSync.lstatSync(at).isSymbolicLink() ? path.resolve(dir, fsSync.readlinkSync(at)) : null; }
    catch { out[name] = null; }
  }
  return out;
}

// The installed version a server runs, from where the engine link in links
// points; null for a manual deploy or a server not set up yet.
export function runningVersion(root, links = root) {
  const engine = currentLinks(links, ["engine"]).engine;
  if (!engine) return null;
  const relative = path.relative(versionsDir(root), engine).split(path.sep);
  return relative.length > 1 && parseVersion(relative[0]) ? relative[0] : null;
}

// ---- Game data ----

function userIds(user) {
  if (!user || process.getuid?.() !== 0) return {};
  const id = (flag) => Number(spawnSync("id", [flag, user], { encoding: "utf8" }).stdout.trim());
  const uid = id("-u"), gid = id("-g");
  return Number.isInteger(uid) && Number.isInteger(gid) && uid > 0 ? { uid, gid } : {};
}

// Runs a release's tools/prepare.py from its own toolchain into a new,
// empty folder; resolves the folder's prepared state. Progress lines go to
// the log, everything to <folder>/prepare.log.
export async function runPreparation({ version, retail, out, jobs, user, spawnImpl = spawn }) {
  const tools = path.join(version, "resources", "tools"), toolchain = path.join(version, "resources", "toolchain");
  const python = path.join(toolchain, "python", "bin", "python3");
  if (!exists(path.join(tools, "prepare.py")) || !exists(python)) throw Error("The release has no preparation tools");
  const ids = userIds(user);
  await fs.mkdir(out, { recursive: true });
  if (ids.uid) await fs.chown(out, ids.uid, ids.gid);
  const env = {
    PATH: [path.join(toolchain, "bin"), "/usr/local/bin", "/usr/bin", "/bin"].join(":"),
    HOME: out, LANG: "C.UTF-8",
    BFE_GAME_DIR: retail, BFE_CONTENT_DIR: out, KTX2_ENCODER: "off",
    PYTHONUNBUFFERED: "1", PYTHONDONTWRITEBYTECODE: "1", PYTHONIOENCODING: "utf-8",
  };
  const logFile = fsSync.createWriteStream(path.join(out, "prepare.log"), { flags: "a" });
  const code = await new Promise((resolve, reject) => {
    const child = spawnImpl(python, ["-u", path.join(tools, "prepare.py"), "--core-only", "--jobs", String(jobs)], { cwd: tools, env, ...ids, stdio: ["ignore", "pipe", "pipe"] });
    let buffer = "";
    const line = (text) => {
      if (!text.startsWith("@@")) return;
      try {
        const event = JSON.parse(text.slice(2));
        if (event.event === "step" && event.status !== "start") log(`prepare ${event.key}: ${event.status}${event.seconds ? ` (${event.seconds} s)` : ""}`);
        else if (event.event === "error" || event.event === "busy") log(`prepare: ${event.message}`);
      } catch {}
    };
    const read = (chunk) => {
      logFile.write(chunk);
      buffer += chunk;
      const lines = buffer.split(/\r?\n/);
      buffer = lines.pop();
      lines.forEach(line);
    };
    child.stdout.setEncoding("utf8").on("data", read);
    child.stderr.setEncoding("utf8").on("data", read);
    child.on("error", reject);
    child.on("close", resolve);
  });
  await new Promise((resolve) => logFile.end(resolve));
  let state = null;
  try { state = JSON.parse(await fs.readFile(path.join(out, ".prepare.json"), "utf8")); } catch {}
  if (code !== 0 || !state || !Array.isArray(state.failed) || state.failed.length || state.skipped?.length) {
    throw Error(`Preparation failed (exit ${code}${state?.failed?.length ? `; ${state.failed.join(", ")}` : ""}); see ${path.join(out, "prepare.log")}`);
  }
  return state;
}

// The release's game data: shared with any release of the same inputs, or
// prepared now. Prepared data belongs to root and is readable by all, so
// the server (its own user) reads but never changes it.
export async function ensureData({ root, versionDir, key, config, state, prepare = runPreparation }) {
  const dir = path.join(root, "data", key);
  if (exists(path.join(dir, DATA_RECORD))) return dir;
  const failures = state.prepareFailures[key] ?? 0;
  if (failures >= MAX_PREPARE_ATTEMPTS) throw Error(`Preparation of this release's game data failed ${failures} times; waiting for a newer release`);
  if (!exists(path.join(config.retail, "GameData"))) throw Error(`No owned game to prepare from at ${config.retail} (the folder holding GameData)`);
  const staging = path.join(root, "data", `.staging-${key}-${process.pid}`);
  await fs.rm(staging, { recursive: true, force: true });
  log(`preparing game data ${key} with ${path.basename(versionDir)}'s tools from ${config.retail}`);
  const started = Date.now();
  try {
    await prepare({ version: versionDir, retail: config.retail, out: staging, jobs: config.jobs, user: config.user });
  } catch (error) {
    state.prepareFailures[key] = failures + 1;
    // The log outlives the half-prepared folder.
    const kept = path.join(root, "data", `failed-${key}.log`);
    await fs.rename(path.join(staging, "prepare.log"), kept).catch(() => {});
    await fs.rm(staging, { recursive: true, force: true }).catch(() => {});
    throw Error(error.message.replace(path.join(staging, "prepare.log"), kept));
  }
  if (process.getuid?.() === 0) {
    spawnSync("chown", ["-R", "0:0", staging]);
    spawnSync("chmod", ["-R", "a+rX,go-w", staging]);
  }
  await fs.writeFile(path.join(staging, DATA_RECORD), JSON.stringify({ key, preparedBy: path.basename(versionDir), preparedAt: new Date().toISOString(), seconds: Math.round((Date.now() - started) / 1000) }, null, 2) + "\n");
  await fs.rename(staging, dir);
  delete state.prepareFailures[key];
  log(`game data ${key} prepared in ${Math.round((Date.now() - started) / 60000)} min`);
  return dir;
}

// The battles a server plays: its rotation, as queries.
export function rotationQueries(stateDir) {
  try {
    const config = JSON.parse(fsSync.readFileSync(path.join(stateDir, "config.json"), "utf8"));
    return (config.rotation ?? []).map((entry) => /^swbf1\/([a-z0-9_-]+):([a-z0-9_-]+)/i.exec(entry)).filter(Boolean).map((match) => `?map=${match[1]}&side=${match[2]}`);
  } catch {
    return ["?map=nab2&side=rep"];
  }
}

// The release's network contract, once every battle it is to play has
// fingerprinted on the data (a missing or unreadable file stops it here,
// not after the swap).
export async function checkRelease({ engine, data, queries }) {
  const { NETWORK_CONTRACT } = await import(pathToFileURL(path.join(engine, "src", "runtime", "network-contract.js")).href);
  const { contentHash } = await import(pathToFileURL(path.join(engine, "src", "runtime", "content-hash.js")).href);
  const fetcher = async (name) => {
    try { return new Response(await fs.readFile(path.join(data, name))); }
    catch (error) { if (error.code === "ENOENT") return new Response("", { status: 404 }); throw error; }
  };
  for (const query of new Set(queries)) await contentHash(query, fetcher);
  return NETWORK_CONTRACT;
}

// ---- The server ----

async function getJson(url, fetchImpl, timeoutMs = 3000) {
  const response = await fetchImpl(url, { signal: AbortSignal.timeout(timeoutMs) });
  if (!response.ok) throw Error(`${url} answered ${response.status}`);
  return response.json();
}

// How many players are on the server: a number, or null when that cannot be
// told (between battles its game port does not answer while players wait).
// A server that is not running has nobody on it.
export async function playersOnServer({ config, fetchImpl = fetch, tries = 3, pauseMs = 3000 }) {
  for (let attempt = 1; attempt <= tries; attempt++) {
    try {
      const info = await getJson(config.info, fetchImpl);
      if (Number.isInteger(info.players)) return info.players;
    } catch {}
    try {
      const health = await getJson(config.health, fetchImpl);
      if (health.status === "fault" || health.status === "setup") return 0;
    } catch {
      if (attempt === tries) return 0;
    }
    if (attempt < tries) await new Promise((resolve) => setTimeout(resolve, pauseMs));
  }
  return null;
}

export async function waitHealthy({ config, contract, fetchImpl = fetch, timeoutMs = config.healthTimeoutMs, pauseMs = 1000 }) {
  const deadline = Date.now() + timeoutMs;
  let last = null, seen = null;
  while (Date.now() < deadline) {
    const health = await getJson(config.health, fetchImpl).catch(() => null);
    if (health) seen = health;
    if (health?.status === "fault") throw Error(`The server reports a fault: ${health.error}`);
    if (health?.ok && health.status === "running" && health.multiplayerReady && JSON.stringify(health.networkContract) === JSON.stringify(contract)) {
      if (last !== null && health.tick > last) return health;
      last = health.tick;
    }
    await new Promise((resolve) => setTimeout(resolve, pauseMs));
  }
  throw Error(`The server did not become healthy${seen ? ` (last: ${seen.status}${seen.error ? `, ${seen.error}` : ""})` : ""}`);
}

const systemctl = (action, service) => new Promise((resolve, reject) => {
  const child = spawn("systemctl", [action, service], { stdio: "inherit" });
  child.on("error", reject);
  child.on("close", (code) => (code === 0 ? resolve() : reject(Error(`systemctl ${action} ${service} failed (${code})`))));
});

// Points a link at target in one rename. A folder of its own in the link's
// place (a manual deploy) is kept beside it as <name>.manual.
function pointLink(link, target) {
  if (lexists(link) && !fsSync.lstatSync(link).isSymbolicLink()) {
    const kept = `${link}.manual`;
    fsSync.rmSync(kept, { recursive: true, force: true });
    fsSync.renameSync(link, kept);
  }
  const temp = `${link}.${process.pid}.link`;
  fsSync.rmSync(temp, { force: true });
  fsSync.symlinkSync(target, temp);
  fsSync.renameSync(temp, link);
}

// Whether the updater in dir runs: --status through a link, as
// bfeserver-update.service starts <root>/update/server.js, must print the
// state.
export function updaterAnswers({ root, dir, config, node = process.execPath }) {
  if (!exists(path.join(dir, "server.js"))) return false;
  const link = path.join(root, `.update-check-${process.pid}`);
  try {
    fsSync.rmSync(link, { force: true });
    fsSync.symlinkSync(dir, link);
    const result = spawnSync(node, [path.join(link, "server.js"), "--status", ...(config.configFile ? [`--config=${config.configFile}`] : [])], { encoding: "utf8", timeout: 30000 });
    return result.status === 0 && "links" in JSON.parse(result.stdout);
  } catch {
    return false;
  } finally {
    fsSync.rmSync(link, { force: true });
  }
}

// What the links point at now, to put back: a target, or the kept manual
// folder.
function restoreLinks(dir, previous) {
  for (const [name, target] of Object.entries(previous)) {
    const link = path.join(dir, name);
    if (target) pointLink(link, target);
    else if (exists(`${link}.manual`)) {
      fsSync.rmSync(link, { force: true });
      fsSync.renameSync(`${link}.manual`, link);
    }
  }
}

// Stops a server, moves its links to the version and its data, starts it
// and waits for health; on failure puts everything back and starts that.
export async function swap({ instance, versionDir, dataDir, contract, control = systemctl, healthy = waitHealthy }) {
  const previous = currentLinks(instance.links, ["engine", "server", "game-data"]);
  const targets = { engine: path.join(versionDir, RUNTIME.engine), server: path.join(versionDir, RUNTIME.server), "game-data": dataDir };
  await fs.mkdir(instance.links, { recursive: true });
  await control("stop", instance.service);
  try {
    for (const [name, target] of Object.entries(targets)) pointLink(path.join(instance.links, name), target);
    await control("start", instance.service);
    return await healthy({ config: instance, contract });
  } catch (error) {
    log(`${instance.name}: rolling back: ${error.message}`);
    await control("stop", instance.service).catch(() => {});
    restoreLinks(instance.links, previous);
    await control("start", instance.service).catch((startError) => log(`${instance.name}: restarting the previous release failed: ${startError.message}`));
    throw error;
  }
}

// The timer runs the updater <root>/update points at: a release's, once it
// answers through a link as the timer starts it. One that does not (or a
// release from before it shipped) leaves the one there, so a broken updater
// can never strand the servers on a release.
function moveUpdater({ root, versionDir, config }) {
  const dir = path.join(versionDir, RUNTIME.update);
  if (currentLinks(root, ["update"]).update === dir) return;
  if (updaterAnswers({ root, dir, config })) pointLink(path.join(root, "update"), dir);
  else log("keeping the current updater: the release's does not answer");
}

// Versions and game data no server runs, nor ran before its last update
// (keepVersions), go; so do a manual deploy's folders (dirs: the link
// folders of servers whose replacement of one has been replaced too).
export async function cleanup({ root, keepVersions, links = [root], manualDone = [] }) {
  const keep = new Set(keepVersions.filter(Boolean));
  const dataInUse = new Set();
  for (const version of keep) {
    try { dataInUse.add(JSON.parse(fsSync.readFileSync(path.join(versionsDir(root), version, VERSION_DATA), "utf8")).key); } catch {}
  }
  for (const dir of links) {
    const data = currentLinks(dir, ["game-data"])["game-data"];
    if (data) dataInUse.add(path.basename(data));
  }
  for (const { version, dir } of installedVersions(root)) if (!keep.has(version)) await fs.rm(dir, { recursive: true, force: true });
  for (const name of await fs.readdir(path.join(root, "data")).catch(() => [])) {
    const staging = /^\.staging-.+-(\d+)$/.exec(name);
    if (staging ? !alive(Number(staging[1])) : !dataInUse.has(name)) await fs.rm(path.join(root, "data", name), { recursive: true, force: true });
  }
  // A manual deploy's folders, kept for rolling back the first release
  // that replaced them, go once that release is replaced too.
  for (const dir of manualDone) for (const name of ["engine", "server", "game-data", "update"]) await fs.rm(path.join(dir, `${name}.manual`), { recursive: true, force: true });
}

const alive = (pid) => {
  try { process.kill(pid, 0); return true; } catch (error) { return error.code === "EPERM"; }
};

// One run at a time, whoever starts it (the timer, or by hand).
function takeLock(root) {
  const file = path.join(root, LOCK);
  for (let attempt = 0; attempt < 2; attempt++) {
    try {
      fsSync.writeFileSync(file, String(process.pid), { flag: "wx" });
      return () => fsSync.rmSync(file, { force: true });
    } catch (error) {
      if (error.code !== "EEXIST") throw error;
      const holder = Number(fsSync.readFileSync(file, "utf8"));
      if (Number.isInteger(holder) && holder > 0 && alive(holder)) return null;
      fsSync.rmSync(file, { force: true });
    }
  }
  return null;
}

// ---- One run ----

export async function updateOnce({ config: given = DEFAULTS, fetchImpl = fetch, keys = UPDATE_KEYS, platform = platformKey(), control = systemctl, prepare = runPreparation, healthy = waitHealthy, players = playersOnServer, reboot = rebootSystem } = {}) {
  const config = { ...DEFAULTS, ...given }, root = config.root;
  if (config.hold) { log("held (hold: true in the configuration)"); return { action: "held" }; }
  await fs.mkdir(path.join(root, "data"), { recursive: true });
  const unlock = takeLock(root);
  if (!unlock) { log("another update run is in progress"); return { action: "busy" }; }
  try { return await updateLocked({ config, fetchImpl, keys, platform, control, prepare, healthy, players, reboot }); }
  finally { unlock(); }
}

async function updateLocked({ config, fetchImpl, keys, platform, control, prepare, healthy, players, reboot }) {
  const root = config.root;
  const state = readUpdateState(root);
  const instances = instancesOf(config);
  const own = (instance) => (state.instances[instance.name] ??= {});
  state.lastCheck = new Date().toISOString();
  try {
    const release = await fetchRelease({ channel: config.channel, keys, fetch: fetchImpl, base: config.feed });
    if (!release) { log(`the ${config.channel} channel has no release`); return { action: "none" }; }
    // The servers behind it: older, a manual deploy, or not set up yet.
    const behind = instances.filter((instance) => {
      const running = runningVersion(root, instance.links);
      return !running || isNewer(release.version, running);
    });
    for (const instance of instances) if (!behind.includes(instance)) delete own(instance).pending;
    if (!behind.length) return { action: (await restartIfRequired({ config, instances, state, players, fetchImpl, reboot })) ? "rebooting" : "current", version: release.version };
    if (state.bad.includes(release.version)) { log(`${release.version} failed here before; waiting for a newer release`); return { action: "bad", version: release.version }; }

    // 1. Install.
    const tree = await fetchTree({ release, platform, fetch: fetchImpl, base: config.feed });
    if (!tree) throw Error(`${release.version} has no ${platform} build`);
    const wanted = serverTree(tree), key = dataKey(wanted);
    const sources = installedVersions(root).map((item) => item.dir);
    const installed = await installRelease({ root, release, tree: wanted, sources, fetch: fetchImpl, base: config.feed, platform: "linux" });
    const versionDir = installed.dir;
    if (installed.downloaded) log(`installed ${release.version} (${Math.round(installed.downloaded / 1e6)} MB downloaded)`);
    const revisionFile = path.join(versionDir, RUNTIME.engine, "engine-revision.json");
    if (release.revision && !exists(revisionFile)) await fs.writeFile(revisionFile, JSON.stringify({ revision: release.revision }));

    // 2. Game data.
    const dataDir = await ensureData({ root, versionDir, key, config, state, prepare });
    await fs.writeFile(path.join(versionDir, VERSION_DATA), JSON.stringify({ key }) + "\n");

    // 3. Check every battle any server plays (once per release and data:
    // runs that wait for empty servers keep what the first found).
    const checked = state.checked?.version === release.version && state.checked.key === key ? state.checked.contract : null;
    const contract = checked ?? await checkRelease({ engine: path.join(versionDir, RUNTIME.engine), data: dataDir, queries: instances.flatMap((instance) => rotationQueries(instance.stateDir)) });
    state.checked = { version: release.version, key, contract };

    // 4. Move each server once nobody plays on it.
    const moved = [], waiting = [];
    for (const instance of behind) {
      const record = own(instance), running = runningVersion(root, instance.links);
      // A server without its configuration would never come up: it is not
      // the release's fault, so it waits rather than condemning it.
      if (!exists(path.join(instance.stateDir, "config.json"))) {
        log(`${instance.name}: no config.json in ${instance.stateDir}; not starting it`);
        waiting.push(instance.name);
        continue;
      }
      const count = await players({ config: instance, fetchImpl });
      if (count !== 0) {
        record.pending = { version: release.version, since: record.pending?.version === release.version ? record.pending.since : new Date().toISOString() };
        log(`${instance.name}: ${release.version} is ready; waiting for an empty server (${count === null ? "players unknown" : `${count} playing`})`);
        waiting.push(instance.name);
        continue;
      }
      log(`${instance.name}: updating ${running ?? (lexists(path.join(instance.links, "engine")) ? "a manual deploy" : "a new server")} to ${release.version}`);
      try {
        await swap({ instance, versionDir, dataDir, contract, control, healthy });
      } catch (error) {
        state.bad = [...new Set([...state.bad, release.version])];
        throw error;
      }
      Object.assign(record, { previous: running, current: release.version, updatedAt: new Date().toISOString() });
      delete record.pending;
      moved.push(instance.name);
      log(`${instance.name}: running ${release.version} (${release.revision ?? "no revision"})`);
    }
    if (moved.length) {
      delete state.lastError;
      moveUpdater({ root, versionDir, config });
    }
    // Keep what any server runs, ran before its last update, or waits for.
    const keepVersions = [release.version, ...instances.flatMap((instance) => [runningVersion(root, instance.links), own(instance).previous])];
    const manualDone = instances.filter((instance) => own(instance).previous).map((instance) => instance.links);
    await cleanup({ root, keepVersions, links: instances.map((instance) => instance.links), manualDone }).catch((error) => log(`cleanup: ${error.message}`));
    await restartIfRequired({ config, instances, state, players, fetchImpl, reboot });
    return { action: moved.length ? "updated" : "waiting", version: release.version, moved, waiting };
  } catch (error) {
    state.lastError = { at: new Date().toISOString(), message: error.message };
    log(error.message);
    throw error;
  } finally {
    writeUpdateState(root, state);
  }
}

// ---- The machine ----

const rebootSystem = () => systemctl("reboot", "--no-wall");

// Unattended upgrades install a kernel or a C library that only a restart
// puts to use, and say so in rebootFlag. With rebootWhenRequired the machine
// restarts once every server is empty, at most once in six hours; the
// servers and this timer start again with it.
export async function restartIfRequired({ config, instances, state, players, fetchImpl, reboot }) {
  if (!config.rebootWhenRequired || !exists(config.rebootFlag)) return false;
  if (state.lastReboot && Date.now() - Date.parse(state.lastReboot) < 6 * 3600 * 1000) return false;
  for (const instance of instances) {
    const count = await players({ config: instance, fetchImpl });
    if (count !== 0) { log(`the system asks for a restart; waiting for empty servers (${instance.name}: ${count ?? "unknown"})`); return false; }
  }
  let packages = "";
  try { packages = fsSync.readFileSync(`${config.rebootFlag}.pkgs`, "utf8").split(/\s+/).filter(Boolean).join(", "); } catch {}
  state.lastReboot = new Date().toISOString();
  writeUpdateState(config.root, state);
  log(`restarting the system for its updates${packages ? ` (${packages})` : ""}`);
  await reboot();
  return true;
}

// ---- Installation ----

// The systemd units (and a configuration, if there is none) for a server
// laid out as DEFAULTS describes; the timer runs the updater the current
// release carries (<root>/update, which each update moves).
export function units({ root = DEFAULTS.root, node = process.execPath, configFile = "/etc/bfeserver-update.json", instances = [], user = DEFAULTS.user } = {}) {
  return {
    ...Object.fromEntries(instances.filter((instance) => instance.unit).map((instance) => [`${instance.service}.service`, serverUnit({ instance, node, user })])),
    "bfeserver-update.service": `[Unit]
Description=Battlefront Eternal dedicated server updates (docs/server-updates.md)
Wants=network-online.target
After=network-online.target

[Service]
Type=oneshot
ExecStart=${node} ${path.join(root, "update", "server.js")} --config=${configFile}
# Preparing game data takes a core or two for most of an hour: never at
# the game's expense.
Nice=19
IOSchedulingClass=idle
CPUWeight=20
MemoryMax=1800M
TimeoutStartSec=6h
`,
    "bfeserver-update.timer": `[Unit]
Description=Check for Battlefront Eternal server updates

[Timer]
OnBootSec=2min
OnUnitInactiveSec=5min
RandomizedDelaySec=20s

[Install]
WantedBy=timers.target
`,
  };
}

// A server's unit, as the main one is set up: its own state folder, health
// port and memory, the engine and server its links point at, restarted
// whenever it stops, started at boot.
export function serverUnit({ instance, node, user }) {
  if (!Number.isInteger(instance.healthPort)) throw Error(`Instance ${instance.name} needs a healthPort for its unit`);
  return `[Unit]
Description=Battlefront Eternal dedicated server (${instance.name})
Wants=network-online.target
After=network-online.target

[Service]
User=${user}
Group=${user}
Environment=BFE_STATE_DIR=${instance.stateDir}
Environment=PORT=${instance.healthPort}
WorkingDirectory=${path.join(instance.links, "server")}
ExecStart=${node} ${path.join(instance.links, "server", "src", "main.js")} --serve
Restart=always
RestartSec=5
MemoryMax=${instance.memoryMax}
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=${instance.stateDir}
PrivateTmp=true

[Install]
WantedBy=multi-user.target
`;
}

function loadConfig(file) {
  let own = {};
  if (file && exists(file)) own = JSON.parse(fsSync.readFileSync(file, "utf8"));
  return { ...DEFAULTS, ...own, configFile: file };
}

async function main(args) {
  const option = (name) => args.find((arg) => arg.startsWith(`--${name}=`))?.slice(name.length + 3);
  const configFile = option("config") ?? "/etc/bfeserver-update.json";
  const config = loadConfig(configFile);
  if (args.includes("--status")) {
    const instances = instancesOf(config).map((instance) => ({ name: instance.name, service: instance.service, running: runningVersion(config.root, instance.links), links: currentLinks(instance.links, ["engine", "server", "game-data"]) }));
    console.log(JSON.stringify({ running: runningVersion(config.root), links: currentLinks(config.root), servers: instances, ...readUpdateState(config.root) }, null, 2));
    return 0;
  }
  if (args.includes("--install-units")) {
    const instances = instancesOf(config);
    for (const [name, text] of Object.entries(units({ root: config.root, node: process.execPath, configFile, instances, user: config.user }))) fsSync.writeFileSync(path.join("/etc/systemd/system", name), text);
    if (!exists(configFile)) fsSync.writeFileSync(configFile, JSON.stringify({ channel: config.channel, retail: config.retail, jobs: config.jobs }, null, 2) + "\n");
    // Each other server's state folder, its own user's alone, and the
    // folder its links go in; the first update run starts it.
    const ids = userIds(config.user);
    for (const instance of instances.filter((item) => item.unit)) {
      fsSync.mkdirSync(instance.links, { recursive: true });
      if (!exists(instance.stateDir)) {
        fsSync.mkdirSync(instance.stateDir, { recursive: true, mode: 0o700 });
        if (ids.uid) fsSync.chownSync(instance.stateDir, ids.uid, ids.gid);
      }
    }
    const enable = instances.filter((item) => item.unit).map((item) => `${item.service}.service`);
    for (const command of [["daemon-reload"], ["enable", "--now", "bfeserver-update.timer"], ...(enable.length ? [["enable", ...enable]] : [])]) spawnSync("systemctl", command, { stdio: "inherit" });
    log(`installed bfeserver-update.timer${enable.length ? ` and ${enable.join(", ")}` : ""}`);
    return 0;
  }
  try {
    await updateOnce({ config });
    return 0;
  } catch {
    return 1;
  }
}

// Run as a program, also through a link (<root>/update is one): Node names
// the module by its real path.
const entry = process.argv[1] ? (() => { try { return fsSync.realpathSync(process.argv[1]); } catch { return null; } })() : null;
if (entry === fileURLToPath(import.meta.url)) process.exitCode = await main(process.argv.slice(2));
