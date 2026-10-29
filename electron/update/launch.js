// Starting another installed version: the hand-off at boot, and the restart
// into a freshly installed update. The new process gets this one's
// arguments and a token; once its window is up it leaves a marker
// (signalReady), and this process waits for that, or for the new process
// to die early, before getting out of the way.
import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { existsSync, mkdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

// An AppImage's runtime points PATH, libraries and data at its own mount,
// which disappears when this process exits: none of that may reach the
// version started from here. The AppImage stays known as BFE_APPIMAGE (the
// desktop entry and game data beside it still use it).
const APPIMAGE_PATHS = ["PATH", "LD_LIBRARY_PATH", "XDG_DATA_DIRS", "GSETTINGS_SCHEMA_DIR"];
const APPIMAGE_VARIABLES = ["APPDIR", "APPIMAGE", "ARGV0", "OWD"];

export function childEnvironment(env, { execDir, platform = process.platform } = {}) {
  const next = { ...env };
  delete next.ELECTRON_RUN_AS_NODE;
  delete next.BFE_LAUNCH_TOKEN;
  delete next.BFE_LAUNCH_DIR;
  delete next.BFE_STEAM_PARENT;
  const appDir = env.APPDIR;
  if (platform === "linux" && appDir) {
    const inside = (entry) => entry === appDir || entry.startsWith(appDir.endsWith("/") ? appDir : appDir + "/");
    for (const name of APPIMAGE_PATHS) {
      if (next[name] === undefined) continue;
      const kept = next[name].split(":").filter((entry) => entry && !inside(entry));
      if (kept.length) next[name] = kept.join(":");
      else delete next[name];
    }
    if (env.APPIMAGE) next.BFE_APPIMAGE = env.APPIMAGE;
    for (const name of APPIMAGE_VARIABLES) delete next[name];
  }
  // Where user namespaces are off, a system package's Chromium sandbox is
  // its setuid helper; a version in the user's folder cannot have its own,
  // and Chromium takes this one instead (CHROME_DEVEL_SANDBOX).
  if (platform === "linux" && execDir && !next.CHROME_DEVEL_SANDBOX) {
    const helper = path.join(execDir, "chrome-sandbox");
    try {
      const stat = statSync(helper);
      if (stat.uid === 0 && stat.mode & 0o4000) next.CHROME_DEVEL_SANDBOX = helper;
    } catch {}
  }
  return next;
}

// Called by the started version when its window is up.
export function signalReady(env = process.env) {
  const token = env.BFE_LAUNCH_TOKEN, dir = env.BFE_LAUNCH_DIR;
  if (!token || !dir || !/^[a-f0-9]{32}$/.test(token)) return false;
  try {
    writeFileSync(path.join(dir, token), String(process.pid));
    return true;
  } catch {
    return false;
  }
}

// Starts `executable` and resolves {started: true} once it signals its
// window, exits cleanly (another copy was already open) or is still
// running after `timeoutMs` (a slow machine); {started: false, code,
// signal | error} if it died first. With `stay`, a started result also
// carries `exited`, a promise of the new process's exit code: whatever
// started this one (Steam, on Windows) watches this process, not the new
// one, so this one stays until the game ends.
export function launchVersion({ executable, args = [], env, root, cwd, timeoutMs = 30000, spawnImpl = spawn, stay = false }) {
  const token = randomBytes(16).toString("hex"), dir = path.join(root, "launches"), marker = path.join(dir, token);
  mkdirSync(dir, { recursive: true });
  let child;
  try {
    child = spawnImpl(executable, args, { cwd, detached: true, stdio: "ignore", env: { ...env, BFE_LAUNCH_TOKEN: token, BFE_LAUNCH_DIR: dir } });
  } catch (error) {
    return Promise.resolve({ started: false, error });
  }
  const exited = new Promise((resolve) => child.once("exit", (code, signal) => resolve(signal ? 1 : code ?? 0)));
  return new Promise((resolve) => {
    let finished = false;
    const finish = (result) => {
      if (finished) return;
      finished = true;
      clearInterval(poll);
      clearTimeout(timer);
      rmSync(marker, { force: true });
      if (result.started && stay && !result.exited) result.exited = exited;
      else if (result.started) child.unref?.();
      resolve(result);
    };
    child.once("error", (error) => finish({ started: false, error }));
    child.once("exit", (code, signal) => finish(code === 0 && !signal ? { started: true, exited: Promise.resolve(0) } : { started: false, code, signal }));
    const poll = setInterval(() => { if (existsSync(marker)) finish({ started: true }); }, 100);
    const timer = setTimeout(() => finish({ started: true, timedOut: true }), timeoutMs);
  });
}
