// What the Steam library entry runs: the same start the desktop launcher
// makes, which hands off to the newest installed update (update/), so the
// entry never needs changing when the game updates.
//
// - Windows: the installed executable.
// - Debian package: /opt/Battlefront Eternal/battlefront-eternal.
// - AppImage, installed by install.sh or registered by the app itself:
//   its launcher in ~/.local/bin, which starts the newest update (and
//   falls back to --no-sandbox where user namespaces are off).
// - A Flatpak Steam cannot see the host's programs: it asks the host to
//   start the launcher (flatpak-spawn --host), which needs the permission
//   to talk to org.freedesktop.Flatpak (docs/presence.md).
import { accessSync, constants, existsSync, readFileSync, readlinkSync, realpathSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { quoted, STEAM_ARG } from "./shortcut.js";
import { updateRoot } from "../update/state.js";

const real = (file) => { try { return realpathSync(file); } catch { return path.resolve(file); } };
const inside = (file, dir) => !!dir && (file === dir || file.startsWith(dir.endsWith(path.sep) ? dir : dir + path.sep));

// A launcher script this app or its installer wrote (they say so).
function ownScript(file) {
  try {
    return /Battlefront Eternal/.test(readFileSync(file, "utf8").slice(0, 512));
  } catch {
    return false;
  }
}

// The command that starts the game as the player's own launcher does, or
// null (a development run, or a build started from somewhere unknown).
// { file, startDir }.
export function gameLauncher({ platform = process.platform, env = process.env, execPath = process.execPath, home = os.homedir(), packaged = true } = {}) {
  if (env.BFE_STEAM_LAUNCHER) return { file: env.BFE_STEAM_LAUNCHER, startDir: path.dirname(env.BFE_STEAM_LAUNCHER) };
  if (!packaged) return null;
  const root = updateRoot({ platform, env, home });
  const bootstrap = env.BFE_BOOTSTRAP_DIR ? path.join(env.BFE_BOOTSTRAP_DIR, path.basename(execPath)) : real(execPath);
  if (platform === "win32") {
    if (inside(bootstrap, root)) return null;
    return existsSync(bootstrap) ? { file: bootstrap, startDir: path.dirname(bootstrap) } : null;
  }
  if (platform !== "linux") return null;
  const appImage = env.APPIMAGE || env.BFE_APPIMAGE || null;
  // Started from the user's own folder (an update, the unpacked AppImage)
  // or from an AppImage: through the launcher in ~/.local/bin.
  if (appImage || inside(bootstrap, root)) {
    for (const name of ["battlefront-eternal", "battlefront"]) {
      const file = path.join(home, ".local", "bin", name);
      if (ownScript(file)) return { file, startDir: home };
    }
    if (appImage && existsSync(appImage)) return { file: appImage, startDir: path.dirname(appImage) };
    return null;
  }
  return existsSync(bootstrap) ? { file: bootstrap, startDir: path.dirname(bootstrap) } : null;
}

// The library entry's Exe, StartDir and LaunchOptions for a Steam
// installation's kind ("windows", "native", "flatpak"), or null when that
// Steam cannot start the game ("snap": its confinement hides the host's
// programs). A Flatpak Steam starts in the home folder, which its sandbox
// has too (flatpak-spawn starts the host's program in the same folder).
export function shortcutLaunch(launcher, kind, { home = os.homedir() } = {}) {
  if (!launcher) return null;
  if (kind === "flatpak") {
    return { exe: quoted("/usr/bin/flatpak-spawn"), startDir: quoted(home), launchOptions: `--host --watch-bus ${quoted(launcher.file)} ${STEAM_ARG}` };
  }
  if (kind === "snap" || kind === "mac") return null;
  const dir = launcher.startDir.endsWith(path.sep) || kind !== "windows" ? launcher.startDir : launcher.startDir + path.sep;
  return { exe: quoted(launcher.file), startDir: quoted(dir), launchOptions: STEAM_ARG };
}

// Whether this run was started by Steam: its library entry's argument, or
// Steam's own environment (a game the player added to Steam by hand).
export function startedBySteam({ argv = process.argv, env = process.env } = {}) {
  return argv.includes(STEAM_ARG) || /^\d+$/.test(env.SteamGameId ?? "") || /^\d+$/.test(env.SteamAppId ?? "") && env.SteamAppId !== "0";
}

// Whether the process Steam started must stay until the game ends, rather
// than hand off and exit: Steam on Windows watches only that process, and
// a Flatpak Steam watches flatpak-spawn, which ends with the process it
// started (which passes no Steam environment). Elsewhere on Linux Steam's
// reaper, a child subreaper, follows the version handed off to.
export function steamStays({ argv = process.argv, env = process.env, platform = process.platform } = {}) {
  if (!startedBySteam({ argv, env })) return false;
  return platform !== "linux" || !/^\d+$/.test(env.SteamGameId ?? "");
}

// Started by Steam through an installed executable from before this code
// (it hands off and exits instead of staying): where Steam follows only that
// process, it soon stops showing the game. A reinstall brings a launcher that
// stays.
export function steamLost({ argv = process.argv, env = process.env, platform = process.platform } = {}) {
  return steamStays({ argv, env, platform }) && env.BFE_HANDOFF === "1" && !env.BFE_STEAM_PARENT;
}

// Whether this executable can start itself again: it is there (a package
// upgrade during the start can delete it), and its own link says so too.
// process.execve cannot fail softly: it ends the process.
export function canRestartSelf(execPath = process.execPath) {
  try {
    accessSync(execPath, constants.X_OK);
    return !/ \(deleted\)$/.test(readlinkSync("/proc/self/exe"));
  } catch {
    return false;
  }
}

// Linux: Steam preloads its overlay into every game it starts (also with
// the overlay turned off), and the Vulkan loader adds its overlay layer.
// In Chromium's sandboxed processes the preloaded overlay crashes the GPU
// process and the zygote, and it cannot work over this game anyway
// (overlay.js: no native Wayland, and on X11 no keyboard). The environment
// without them, or null when there is nothing to take out. boot.js then
// starts the same executable again in place (process.execve: the same
// process, which Steam keeps following), before any window or GPU process
// exists.
export const STEAM_LIBRARIES = /\/(ubuntu12_32|ubuntu12_64|steam-runtime)(\/|$)/;

export function withoutSteamOverlay(env = process.env, platform = process.platform) {
  if (platform !== "linux" || env.BFE_STEAM_CLEAN) return null;
  const preload = String(env.LD_PRELOAD ?? "");
  const layers = Object.keys(env).filter((name) => /^ENABLE_VK_LAYER_VALVE_steam_overlay/.test(name));
  if (!/gameoverlayrenderer\.so/.test(preload) && !layers.length) return null;
  const next = { ...env, BFE_STEAM_CLEAN: "1" };
  const kept = preload.split(/[: ]/).filter((entry) => entry && !/gameoverlayrenderer\.so$/.test(entry));
  if (kept.length) next.LD_PRELOAD = kept.join(":");
  else delete next.LD_PRELOAD;
  for (const name of layers) delete next[name];
  // The Steam runtime's libraries, where a client still passes them on.
  if (next.LD_LIBRARY_PATH !== undefined) {
    const paths = next.LD_LIBRARY_PATH.split(":").filter((entry) => entry && !STEAM_LIBRARIES.test(entry));
    if (paths.length) next.LD_LIBRARY_PATH = paths.join(":");
    else delete next.LD_LIBRARY_PATH;
  }
  return next;
}
