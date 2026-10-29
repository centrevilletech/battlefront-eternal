// The desktop updater (docs/desktop-updates.md). On every launch:
//
// 1. handOff (electron/boot.js, before the app loads): if a newer version
//    is installed in the user's update folder, start it instead and exit.
//    The installed package (Debian, AppImage, Windows installer) stays
//    the fallback and never needs administrator rights to update.
// 2. check/install/launch (main.js, under the loader): ask the signed
//    feed for a newer release, install it beside this one and restart
//    into it before the menu opens. Offline or failing, the game starts
//    as it is.
// 3. ready/settle (main.js): the window is up (the version that started
//    this one may exit); a little later the version is healthy, and
//    versions nothing needs are deleted.
//
// Prepared game data, settings and identities live in the user data
// folder, which no version owns: an update never touches them, and
// preparation reruns only when content-version.json says so.
import { appendFileSync, mkdirSync, readFileSync, realpathSync, statSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { FEED, fetchRelease, fetchTree } from "./feed.js";
import { installRelease } from "./install.js";
import { UPDATE_KEYS } from "./keys.js";
import { childEnvironment, launchVersion, signalReady } from "./launch.js";
import { changeState, chooseLaunch, collect, installedVersions, platformKey, pointCurrent, readState, recordFailure, recordHealthy, SUPPORTED_PLATFORMS, updateRoot } from "./state.js";
import { isNewer, parseVersion } from "./version.js";

const real = (file) => { try { return realpathSync(file); } catch { return path.resolve(file); } };

export function updateLog(root) {
  return (message) => {
    if (!root) return;
    try {
      mkdirSync(root, { recursive: true });
      const file = path.join(root, "update.log");
      try { if (statSync(file).size > 1 << 20) writeFileSync(file, ""); } catch {}
      appendFileSync(file, `${new Date().toISOString()} [${process.pid}] ${message}\n`);
    } catch {}
  };
}

// The arguments a restart passes on: the user's, without Electron's own
// first entry.
const passedArguments = (argv) => argv.slice(1);
const workingDirectory = (env) => (env.APPDIR && process.cwd().startsWith(env.APPDIR) ? os.homedir() : process.cwd());

// Step 1. Resolves true when another version took over (exit now). A
// version that dies before its window is counted and the next newest one
// tried, down to this executable itself: the last working build runs.
// With `stay` (started by Steam, which watches this process) it resolves
// the launch, whose `exited` says when to exit and with what.
export async function handOff({ app, env = process.env, execPath = process.execPath, argv = process.argv, platform = process.platform, spawnImpl, stay = false } = {}) {
  if (!app.isPackaged || env.BFE_NO_UPDATE || env.BFE_HANDOFF) return false;
  const root = updateRoot({ platform, env }), own = app.getVersion();
  if (!root || !parseVersion(own)) return false;
  const log = updateLog(root), execDir = path.dirname(real(execPath)), failed = new Set();
  const childEnv = { ...childEnvironment(env, { execDir, platform }), BFE_HANDOFF: "1", BFE_BOOTSTRAP_DIR: env.BFE_BOOTSTRAP_DIR || execDir, BFE_BOOTSTRAP_VERSION: env.BFE_BOOTSTRAP_VERSION || own };
  for (;;) {
    let target;
    try {
      const state = readState(root), choice = chooseLaunch({ own, installed: installedVersions(root).filter((item) => !failed.has(item.version)), state });
      if (choice.setAside.length) changeState(root, (next) => { for (const version of choice.setAside) if (!next.bad.includes(version)) next.bad.push(version); });
      target = choice.target;
    } catch (error) {
      log(`hand-off skipped: ${error.message}`);
      return false;
    }
    if (!target) return false;
    log(`${own} hands off to ${target.version}`);
    // A process that stays for Steam is the one Steam stops: the game
    // follows it (main.js).
    const launchEnv = stay ? { ...childEnv, BFE_STEAM_PARENT: String(process.pid) } : childEnv;
    const result = await launchVersion({ executable: target.executable, args: passedArguments(argv), env: launchEnv, root, cwd: workingDirectory(env), spawnImpl, stay });
    if (result.started) return stay ? result : true;
    failed.add(target.version);
    recordFailure(root, target.version);
    log(`${target.version} did not start (${result.error?.message ?? `exit ${result.code ?? result.signal}`})`);
  }
}

// Until the window is up (ready, below), an error anywhere in the main
// process ends this run with a logged reason instead of Electron's modal
// error dialog: the version that started this one is waiting to see it
// exit and carry on itself, and the next launch counts the failure.
let guard = null;
export function startupGuard({ app, env = process.env, platform = process.platform }) {
  if (!app.isPackaged || guard) return;
  const log = updateLog(updateRoot({ platform, env }));
  guard = (error) => {
    log(`${app.getVersion()} failed while starting: ${error?.stack ?? error}`);
    app.exit(1);
  };
  process.on("uncaughtException", guard);
}
// For an error the entry point caught itself: true if the guard handled it.
export function startupFailed(error) {
  if (!guard) return false;
  guard(error);
  return true;
}
function releaseGuard() {
  if (!guard) return;
  process.off("uncaughtException", guard);
  guard = null;
}

function readChannel(app) {
  try {
    const channel = JSON.parse(readFileSync(path.join(app.getAppPath(), "package.json"), "utf8")).bfeUpdateChannel;
    return typeof channel === "string" && /^[a-z0-9-]{1,40}$/.test(channel) ? channel : null;
  } catch {
    return null;
  }
}

// Steps 2 and 3. `fetch` is Electron's net.fetch (system proxies apply).
export function createUpdater({ app, fetch, env = process.env, platform = process.platform, arch = process.arch, execPath = process.execPath, argv = process.argv, keys = UPDATE_KEYS, base = env.BFE_UPDATE_FEED || FEED, spawnImpl } = {}) {
  const key = platformKey(platform, arch), root = updateRoot({ platform, env }), own = app.getVersion();
  const channel = readChannel(app), log = updateLog(root);
  const ownDir = path.dirname(real(execPath));
  const enabled = app.isPackaged && !!channel && !!root && SUPPORTED_PLATFORMS.has(key) && !!parseVersion(own) && !env.BFE_NO_UPDATE && Object.keys(keys).length > 0;
  const userLocal = () => installedVersions(root).find((item) => real(item.dir) === ownDir) ?? null;

  return {
    enabled, root, channel, version: own, log,

    // A newer release than this one for this platform, or null. Throws
    // when the feed cannot be read (offline): the caller carries on.
    async check({ timeoutMs = 5000, signal } = {}) {
      const limit = AbortSignal.timeout(timeoutMs);
      const both = signal ? AbortSignal.any([signal, limit]) : limit;
      const release = await fetchRelease({ channel, keys, fetch, base, signal: both });
      if (!release || !isNewer(release.version, own)) return null;
      if (readState(root).bad.includes(release.version)) {
        log(`${release.version} failed to start here before; waiting for a newer release`);
        return null;
      }
      const tree = await fetchTree({ release, platform: key, fetch, base, signal: both });
      if (!tree) return null;
      log(`${own}: ${release.version} available (${tree.files.length} files)`);
      return { release, tree };
    },

    // Installs a release found by check(); files this computer already has
    // in any build are reused, the rest downloaded.
    async install({ release, tree }, { signal, onProgress } = {}) {
      const sources = [ownDir, env.BFE_BOOTSTRAP_DIR, ...installedVersions(root).map((item) => item.dir)];
      const begun = Date.now();
      const installed = await installRelease({ root, release, tree, sources, fetch, base, signal, onProgress, platform });
      log(`installed ${release.version}: ${installed.reused} files reused, ${(installed.downloaded / 1e6).toFixed(1)} MB downloaded in ${((Date.now() - begun) / 1000).toFixed(1)} s`);
      return installed;
    },

    // Starts an installed version in this one's place. {started} as in
    // launchVersion (`stay`: with `exited`); a version that dies before its
    // window is counted.
    async launch(installed, { stay = false } = {}) {
      const childEnv = { ...childEnvironment(env, { execDir: env.BFE_BOOTSTRAP_DIR || ownDir, platform }), BFE_HANDOFF: "1", BFE_BOOTSTRAP_DIR: env.BFE_BOOTSTRAP_DIR || ownDir, BFE_BOOTSTRAP_VERSION: env.BFE_BOOTSTRAP_VERSION || own, BFE_UPDATED_FROM: own };
      if (stay) childEnv.BFE_STEAM_PARENT = String(process.pid);
      const result = await launchVersion({ executable: installed.executable, args: passedArguments(argv), env: childEnv, root, cwd: workingDirectory(env), spawnImpl, stay });
      if (!result.started) {
        recordFailure(root, installed.version);
        log(`${installed.version} did not start (${result.error?.message ?? `exit ${result.code ?? result.signal}`})`);
      }
      return result;
    },

    // This version's window is up: the one that started it may exit, and
    // errors from here on are reported as usual.
    ready() {
      releaseGuard();
      return signalReady(env);
    },

    // Some time after the window came up: this version works. Records
    // it, points Linux launchers at it and deletes what nothing needs.
    // `protect`: paths in use elsewhere (a hosted server's runtime).
    // Only a build that installs updates cleans them up: a local or
    // unpackaged build (version 0.1.0, above every preview) would delete
    // the installed client's.
    settle({ protect = [] } = {}) {
      if (!enabled) return [];
      try {
        const installed = userLocal();
        if (installed) {
          recordHealthy(root, own);
          if (platform === "linux") pointCurrent(root, installed.dir);
        }
        const removed = collect(root, { own, protect: [ownDir, ...protect], keepFallback: !!installed });
        if (removed.length) log(`removed ${removed.join(", ")}`);
        return removed;
      } catch (error) {
        log(`settle failed: ${error.message}`);
        return [];
      }
    },
  };
}
