// The Steam client on this computer: its installations (native, Flatpak or
// Snap on Linux; the registry's on Windows), their accounts, which one is
// running and signed in, and the command that talks to it.
import { execFile, spawn } from "node:child_process";
import { existsSync, promises as fs } from "node:fs";
import os from "node:os";
import path from "node:path";
import { regExe, steamRoots } from "../discover.js";
import { childEnvironment } from "../update/launch.js";
import { STEAM_LIBRARIES } from "./launcher.js";
import { parseTextVdf } from "./vdf.js";

// steamid64 = this + account id (userdata/<account id>).
const STEAMID64_BASE = 76561197960265728n;

export const FLATPAK_ID = "com.valvesoftware.Steam";

// What kind of installation a Steam root is.
export function rootKind(root, platform = process.platform) {
  if (platform === "win32") return "windows";
  if (platform === "darwin") return "mac";
  if (root.includes(`/.var/app/${FLATPAK_ID}/`)) return "flatpak";
  if (/\/snap\/steam\//.test(root)) return "snap";
  return "native";
}

// The folder holding a Linux installation's registry.vdf and steam.pid
// (~/.steam, inside the sandbox's home for Flatpak and Snap).
export function dotSteam(kind, home = os.homedir()) {
  if (kind === "flatpak") return path.join(home, ".var", "app", FLATPAK_ID, ".steam");
  if (kind === "snap") return path.join(home, "snap", "steam", "common", ".steam");
  return path.join(home, ".steam");
}

// Every Steam installation: [{ root, kind }].
export async function steamInstalls({ platform = process.platform, home = os.homedir(), env = process.env, roots } = {}) {
  const found = roots ?? (await steamRoots({ platform, home, env }));
  const out = [];
  for (const root of found) {
    if (!existsSync(path.join(root, "userdata")) && !existsSync(path.join(root, "steamapps"))) continue;
    out.push({ root, kind: rootKind(root, platform) });
  }
  return out;
}

// The accounts that signed in to an installation: [{ account, name,
// mostRecent }] from config/loginusers.vdf and the userdata folders.
export async function steamAccounts(root) {
  const accounts = new Map();
  try {
    const users = parseTextVdf(await fs.readFile(path.join(root, "config", "loginusers.vdf"), "utf8")).users ?? {};
    for (const [id, user] of Object.entries(users)) {
      if (!/^\d{17}$/.test(id)) continue;
      const account = String(BigInt(id) - STEAMID64_BASE);
      accounts.set(account, { account, name: String(user.PersonaName || user.AccountName || "").slice(0, 64), mostRecent: user.MostRecent === "1", time: Number(user.Timestamp) || 0 });
    }
  } catch {}
  try {
    for (const entry of await fs.readdir(path.join(root, "userdata"), { withFileTypes: true })) {
      if (entry.isDirectory() && /^[1-9]\d{0,10}$/.test(entry.name) && !accounts.has(entry.name)) accounts.set(entry.name, { account: entry.name, name: "", mostRecent: false, time: 0 });
    }
  } catch {}
  return [...accounts.values()].sort((a, b) => b.mostRecent - a.mostRecent || b.time - a.time);
}

export const shortcutsFile = (root, account) => path.join(root, "userdata", String(account), "config", "shortcuts.vdf");
export const gridDir = (root, account) => path.join(root, "userdata", String(account), "config", "grid");

// `reg query <key>`'s values: { name: string | number (DWORD) }.
export function parseRegQuery(stdout) {
  const values = {};
  for (const match of String(stdout).matchAll(/^\s+(\S+)\s+(REG_\w+)\s+(.*?)\s*$/gm)) {
    values[match[1]] = match[2] === "REG_DWORD" ? Number.parseInt(match[3], 16) : match[3];
  }
  return values;
}

function registryValues(key) {
  return new Promise((resolve) => {
    execFile(regExe(), ["query", key], { windowsHide: true, timeout: 4000 }, (error, stdout) => resolve(error ? {} : parseRegQuery(stdout)));
  });
}

// Linux: the Steam client processes of this user ([{pid, kind}]), from
// /proc: the client is ubuntu12_32/steam (ubuntu12_64 in newer builds),
// and a Flatpak's or Snap's says so in its environment.
export async function linuxSteamProcesses({ proc = "/proc" } = {}) {
  const out = [];
  let names;
  try { names = await fs.readdir(proc); } catch { return out; }
  await Promise.all(names.filter((name) => /^\d+$/.test(name)).map(async (pid) => {
    let exe;
    try { exe = await fs.readlink(path.join(proc, pid, "exe")); } catch { return; }
    if (!/\/ubuntu12_(32|64)\/steam( \(deleted\))?$/.test(exe)) return;
    let environ = "";
    try { environ = (await fs.readFile(path.join(proc, pid, "environ"))).toString("latin1"); } catch {}
    const vars = environ.split("\0");
    const kind = vars.includes(`FLATPAK_ID=${FLATPAK_ID}`) ? "flatpak" : vars.some((item) => item === "SNAP_NAME=steam") ? "snap" : "native";
    out.push({ pid: Number(pid), kind });
  }));
  return out;
}

// The running Steam client: { running, kind, account (signed in, else
// null), pid, game (the Steam game it runs, else 0) } or { running: false }.
export async function runningSteam({ platform = process.platform, home = os.homedir(), proc = "/proc", registry = registryValues } = {}) {
  if (platform === "win32") {
    const active = await registry("HKCU\\Software\\Valve\\Steam\\ActiveProcess");
    const pid = Number(active.pid) || 0;
    if (!pid || !alive(pid)) return { running: false };
    const account = Number(active.ActiveUser) || 0;
    const game = Number((await registry("HKCU\\Software\\Valve\\Steam")).RunningAppID) || 0;
    return { running: true, kind: "windows", pid, account: account ? String(account) : null, game };
  }
  if (platform !== "linux") return { running: false };
  const processes = await linuxSteamProcesses({ proc });
  if (!processes.length) return { running: false };
  const { kind, pid } = processes.sort((a, b) => a.pid - b.pid)[0];
  let account = null, game = 0;
  try {
    const steam = parseTextVdf(await fs.readFile(path.join(dotSteam(kind, home), "registry.vdf"), "utf8"))?.Registry?.HKCU?.Software?.Valve?.Steam;
    const value = Number(steam?.ActiveProcess?.ActiveUser) || 0;
    if (value) account = String(value);
    game = Number(steam?.RunningAppID) || 0;
  } catch {}
  return { running: true, kind, pid, account, game };
}

function alive(pid) {
  try { process.kill(pid, 0); return true; } catch (error) { return error.code === "EPERM"; }
}

// The command that reaches a Steam installation (and a running client
// through it): { file, args } with `args` before the caller's.
export async function steamCommand({ kind, root, platform = process.platform, env = process.env, registry = registryValues } = {}) {
  if (platform === "win32") {
    const values = await registry("HKCU\\Software\\Valve\\Steam");
    const exe = typeof values.SteamExe === "string" && values.SteamExe ? path.normalize(values.SteamExe) : path.join(root, "steam.exe");
    return { file: exe, args: [] };
  }
  if (kind === "flatpak") return { file: "flatpak", args: ["run", FLATPAK_ID] };
  if (kind === "snap") return { file: "snap", args: ["run", "steam"] };
  for (const dir of [...String(env.PATH || "").split(":"), "/usr/bin", "/usr/games", "/usr/local/bin"]) {
    if (dir && existsSync(path.join(dir, "steam"))) return { file: path.join(dir, "steam"), args: [] };
  }
  return { file: path.join(root, "steam.sh"), args: [] };
}

// The environment Steam gets from us: the session's, without this app's
// own variables (a Steam started here passes its environment to every game
// it starts, this one included: BFE_HANDOFF would stop its update hand-off),
// what Electron and Chromium add, an AppImage's mount, and what a Steam
// launch of this game added (its marks, Steam Input's, the runtime's and the
// overlay's).
const NOT_FOR_STEAM = /^(BFE_|ELECTRON_|CHROME_|STEAM_COMPAT_|STEAM_RUNTIME|SDL_GAMECONTROLLER|SDL_JOYSTICK|ENABLE_VK_LAYER_VALVE)|^(NO_AT_BRIDGE|FC_FONTATIONS|SteamAppId|SteamGameId|SteamOverlayGameId|SteamClientLaunch|SteamEnv|SteamNoOverlayUI|SteamAppUser|SteamUser|SteamVirtualGamepadInfo|SteamStreamingHardwareEncoding\w*|EnableConfiguratorSupport|STEAMSCRIPT\w*|STEAM_ZENITY|SYSTEM_PATH|SYSTEM_LD_LIBRARY_PATH)$/i;

export function steamEnvironment(env = process.env, platform = process.platform) {
  const next = childEnvironment(env, { platform });
  for (const name of Object.keys(next)) if (NOT_FOR_STEAM.test(name)) delete next[name];
  if (next.LD_PRELOAD !== undefined) {
    const kept = next.LD_PRELOAD.split(/[: ]/).filter((entry) => entry && !/gameoverlayrenderer\.so$/.test(entry));
    if (kept.length) next.LD_PRELOAD = kept.join(":");
    else delete next.LD_PRELOAD;
  }
  if (next.LD_LIBRARY_PATH !== undefined) {
    const kept = next.LD_LIBRARY_PATH.split(":").filter((entry) => entry && !STEAM_LIBRARIES.test(entry));
    if (kept.length) next.LD_LIBRARY_PATH = kept.join(":");
    else delete next.LD_LIBRARY_PATH;
  }
  return next;
}

// Hands arguments to Steam (a steam:// URL, -shutdown, -silent) and returns
// at once; the command forwards them to a running client and exits, or
// starts Steam.
export function runSteam(command, args, { spawnImpl = spawn, env = process.env, platform = process.platform } = {}) {
  const child = spawnImpl(command.file, [...command.args, ...args], { detached: true, stdio: "ignore", windowsHide: true, env: steamEnvironment(env, platform) });
  child.on?.("error", () => {});
  child.unref?.();
  return child;
}
