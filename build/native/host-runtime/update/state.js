// The per-user update folder (layout below), which versions are installed
// there, which to start, and which to delete. Synchronous on purpose: the
// boot hand-off reads it before the app has loaded anything else.
//
//   <root>/versions/<version>/   complete installs (the record is written last)
//   <root>/versions/.staging-*   installs in progress
//   <root>/state.json            launch attempts, failed and healthy versions
//   <root>/launches/<token>      "my window is up" markers from new launches
//   <root>/current               Linux: link to the newest healthy version
import fs from "./fs.js";
import os from "node:os";
import path from "node:path";
import { compareVersions, parseVersion } from "./version.js";

const { existsSync, lstatSync, mkdirSync, readdirSync, readFileSync, readlinkSync, renameSync, rmSync, statSync, symlinkSync, writeFileSync } = fs;

export const INSTALL_RECORD = ".bfe-install.json";
// Launches that died before their window came up; then the version is
// set aside until a newer one arrives.
export const MAX_ATTEMPTS = 2;
const STALE_MS = 6 * 3600 * 1000;

// Per user, outside anything a system package manager owns, so updating
// never needs administrator rights.
export function updateRoot({ platform = process.platform, env = process.env, home = os.homedir() } = {}) {
  if (platform === "linux") return path.join(env.XDG_DATA_HOME || path.join(home, ".local", "share"), "battlefront-eternal");
  if (platform === "win32") return path.join(env.LOCALAPPDATA || path.join(home, "AppData", "Local"), "Battlefront Eternal");
  return null;
}
export const versionsDir = (root) => path.join(root, "versions");
export const platformKey = (platform = process.platform, arch = process.arch) => `${platform}-${arch}`;
export const SUPPORTED_PLATFORMS = new Set(["linux-x64", "linux-arm64", "win32-x64"]);

// Windows refuses a rename while a scanner holds the file; it lets go soon.
export function renameRetry(from, to) {
  for (let attempt = 0; ; attempt++) {
    try { return renameSync(from, to); }
    catch (error) {
      if (attempt >= 20 || !["EPERM", "EACCES", "EBUSY"].includes(error.code)) throw error;
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 50 * (attempt + 1));
    }
  }
}

export function readState(root) {
  let state = {};
  try { state = JSON.parse(readFileSync(path.join(root, "state.json"), "utf8")) || {}; } catch {}
  const list = (value) => (Array.isArray(value) ? value.filter((item) => parseVersion(item)) : []);
  const attempts = {};
  for (const [version, count] of Object.entries(state.attempts ?? {})) if (parseVersion(version) && Number.isInteger(count)) attempts[version] = count;
  return { attempts, bad: list(state.bad), healthy: list(state.healthy) };
}

export function writeState(root, state) {
  mkdirSync(root, { recursive: true });
  const file = path.join(root, "state.json"), temp = `${file}.${process.pid}.tmp`;
  writeFileSync(temp, JSON.stringify(state, null, 2) + "\n");
  renameRetry(temp, file);
}

export function changeState(root, change) {
  const state = readState(root);
  change(state);
  writeState(root, state);
  return state;
}

// Complete installs, newest first.
export function installedVersions(root) {
  let names;
  try { names = readdirSync(versionsDir(root)); } catch { return []; }
  const found = [];
  for (const name of names) {
    if (!parseVersion(name)) continue;
    const dir = path.join(versionsDir(root), name);
    try {
      const record = JSON.parse(readFileSync(path.join(dir, INSTALL_RECORD), "utf8"));
      const executable = path.join(dir, ...String(record.executable).split("/"));
      if (record.version === name && existsSync(executable)) found.push({ version: name, dir, executable, record });
    } catch {}
  }
  return found.sort((a, b) => compareVersions(b.version, a.version));
}

// The installed version a launch of `own` should start instead: the newest
// one newer than it that has not failed. Returns it (or null) and the
// versions this decision set aside.
export function chooseLaunch({ own, installed, state }) {
  const setAside = [];
  for (const candidate of installed) {
    if (compareVersions(candidate.version, own) <= 0) break;
    if (state.bad.includes(candidate.version)) continue;
    if (!state.healthy.includes(candidate.version) && (state.attempts[candidate.version] ?? 0) >= MAX_ATTEMPTS) {
      setAside.push(candidate.version);
      continue;
    }
    return { target: candidate, setAside };
  }
  return { target: null, setAside };
}

export function recordFailure(root, version) {
  return changeState(root, (state) => {
    state.attempts[version] = (state.attempts[version] ?? 0) + 1;
    if (state.attempts[version] >= MAX_ATTEMPTS && !state.bad.includes(version)) state.bad.push(version);
  });
}

export function recordHealthy(root, version) {
  return changeState(root, (state) => {
    delete state.attempts[version];
    state.bad = state.bad.filter((item) => item !== version);
    state.healthy = [version, ...state.healthy.filter((item) => item !== version)].slice(0, 8);
  });
}

// Linux launchers (the AppImage installer's) start <root>/current directly,
// skipping the AppImage's unpacking.
export function pointCurrent(root, dir) {
  const link = path.join(root, "current"), temp = path.join(root, `.current-${process.pid}`);
  const target = path.relative(root, dir);
  try { if (readlinkSync(link) === target) return false; } catch {}
  try { if (lstatSync(link).isDirectory()) return false; } catch {}
  rmSync(temp, { force: true });
  symlinkSync(target, temp);
  renameSync(temp, link);
  return true;
}

const within = (dir, file) => {
  const relative = path.relative(dir, file);
  return !!relative && !relative.startsWith("..") && !path.isAbsolute(relative);
};

// Deletes installs nothing needs: keeps the running one (`own`), newer ones
// waiting for their first launch, the newest healthy one older than `own`
// (where a failed update falls back to, unless `own` is the installed
// package, which is its own fallback) and any holding a path in
// `protect` (the running executable, a hosted server's runtime). Folders
// left without a record (a deletion Windows interrupted) go too, as do
// abandoned staging folders and stale launch markers. Returns the deleted
// versions.
export function collect(root, { own, protect = [], keepFallback = true, now = Date.now() } = {}) {
  const installed = installedVersions(root), state = readState(root);
  const keep = new Set();
  let fallback = !keepFallback;
  for (const item of installed) {
    const order = compareVersions(item.version, own);
    if (order >= 0) { if (order === 0 || !state.bad.includes(item.version)) keep.add(item.version); }
    else if (!fallback && state.healthy.includes(item.version)) { fallback = true; keep.add(item.version); }
  }
  let names = [];
  try { names = readdirSync(versionsDir(root)); } catch {}
  const removed = [];
  for (const name of names) {
    const dir = path.join(versionsDir(root), name);
    if (!parseVersion(name) || keep.has(name) || protect.some((file) => file === dir || within(dir, file))) continue;
    try {
      // The record first: a half-deleted folder is never taken for an install.
      rmSync(path.join(dir, INSTALL_RECORD), { force: true });
      rmSync(dir, { recursive: true, force: true, maxRetries: 2 });
      if (installed.some((item) => item.version === name)) removed.push(name);
    } catch {}
  }
  for (const [dir, prefix] of [[versionsDir(root), ".staging-"], [path.join(root, "launches"), ""]]) {
    let names = [];
    try { names = readdirSync(dir); } catch {}
    for (const name of names) {
      if (!name.startsWith(prefix)) continue;
      const file = path.join(dir, name);
      try { if (now - statSync(file).mtimeMs > STALE_MS) rmSync(file, { recursive: true, force: true }); } catch {}
    }
  }
  const left = new Set(installedVersions(root).map((item) => item.version));
  changeState(root, (next) => {
    for (const version of Object.keys(next.attempts)) if (!left.has(version)) delete next.attempts[version];
    next.healthy = next.healthy.filter((version) => left.has(version) || version === own);
    next.bad = next.bad.slice(-16);
  });
  // A link to a deleted version would only make launchers fall back late.
  try { if (!existsSync(path.join(root, "current"))) rmSync(path.join(root, "current"), { force: true }); } catch {}
  return removed;
}
