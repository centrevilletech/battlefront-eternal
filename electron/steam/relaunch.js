// A start from outside Steam (the desktop launcher, the Start menu) goes
// through the player's Steam library instead, so Steam knows the game is
// running and friends see "playing Battlefront Eternal". Runs at boot
// (electron/boot.js), before anything else loads, and only when the player
// added the game to Steam from this app, Steam is running and signed in to
// that account, and the library entry is there.
//
// This process asks Steam to start the entry (steam://rungameid/<id>) and
// leaves a request with a token; the copy Steam starts finds the request
// and leaves the token as a marker (arrived), and this one exits. If Steam
// does not start it in time, this one simply carries on (nothing about
// Steam may stop the game from starting), and so do the versions it hands
// off to and the starts of the next half hour.
import { randomBytes } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { runningSteam, runSteam, shortcutsFile, steamCommand } from "./client.js";
import { startedBySteam } from "./launcher.js";
import { describeShortcut, findShortcut, rungameId } from "./shortcut.js";
import { parseShortcuts } from "./vdf.js";

const REQUEST = "steam-relaunch.json", FAILED = "steam-relaunch-failed";
// How long a request stands: Steam can take a while (an update check, a
// slow disk), and a request this old was never answered.
export const REQUEST_MS = 20000;
// After Steam did not start the game, how long starts go directly.
export const BACKOFF_MS = 30 * 60 * 1000;

const age = (file) => Date.now() - statSync(file).mtimeMs;
const alive = (pid) => {
  try { process.kill(pid, 0); return true; } catch (error) { return error.code === "EPERM"; }
};

function readRequest(dir) {
  try {
    const file = path.join(dir, REQUEST);
    const request = JSON.parse(readFileSync(file, "utf8"));
    if (!/^[a-f0-9]{32}$/.test(request?.token ?? "")) return null;
    return { ...request, age: age(file) };
  } catch {
    return null;
  }
}

function failedLately(dir) {
  try { return age(path.join(dir, FAILED)) < BACKOFF_MS; } catch { return false; }
}

// In the copy Steam started: answers a waiting request (true if there was
// one).
export function arrived({ dir, argv = process.argv, env = process.env }) {
  if (!dir || !startedBySteam({ argv, env })) return false;
  const request = readRequest(dir);
  if (!request || request.age > REQUEST_MS) return false;
  try {
    writeFileSync(path.join(dir, request.token), String(process.pid));
    rmSync(path.join(dir, REQUEST), { force: true });
    return true;
  } catch {
    return false;
  }
}

// The library entry Steam would start for the running, signed-in account,
// or null: { steam, command, entry, gameId }.
export async function steamEntry({ steam, platform = process.platform, home, running = runningSteam, command = steamCommand } = {}) {
  if (!steam?.appid || !steam.root || steam.launch === false) return null;
  const now = await running({ platform, home });
  if (!now.running || !now.account || (steam.kind && now.kind !== steam.kind)) return null;
  let list;
  try { list = parseShortcuts(readFileSync(shortcutsFile(steam.root, now.account))); } catch { return null; }
  const index = findShortcut(list.entries, { appid: steam.appid });
  if (index < 0) return null;
  const entry = describeShortcut(list.entries[index]);
  // A Flatpak Steam's entry names the host's launcher in its options.
  const exe = entry.exe.replace(/^"(.*)"$/, "$1");
  if (now.kind !== "flatpak" && !existsSync(exe)) return null;
  return { steam: now, command: await command({ kind: now.kind, root: steam.root, platform }), entry, gameId: rungameId(entry.appid) };
}

// Resolves "steam" when Steam started the game in this one's place, or
// "waiting" when another start is going through Steam right now (a second
// click on the launcher): exit now. false: carry on. `steam`: the desktop
// settings' steam record.
export async function relaunchThroughSteam({ dir, steam, argv = process.argv, env = process.env, platform = process.platform, home, timeoutMs = REQUEST_MS, find = steamEntry, run = runSteam, log = () => {} }) {
  if (!dir || env.BFE_NO_STEAM || startedBySteam({ argv, env })) return false;
  const previous = readRequest(dir);
  // That start, or the copy Steam starts for it, comes up. (A copy Steam
  // started without its marks waits it out too: the start that asked
  // carries on by itself.)
  if (previous && previous.age < timeoutMs && Number.isInteger(previous.pid) && previous.pid !== process.pid && alive(previous.pid)) {
    log("another start is going through Steam");
    return "waiting";
  }
  if (failedLately(dir)) return false;
  const target = await find({ steam, platform, home });
  if (!target) return false;
  const token = randomBytes(16).toString("hex"), marker = path.join(dir, token);
  mkdirSync(dir, { recursive: true });
  writeFileSync(path.join(dir, REQUEST), JSON.stringify({ token, pid: process.pid, gameId: target.gameId }));
  log(`starting through Steam (${target.steam.kind}, ${target.gameId})`);
  try {
    run(target.command, [`steam://rungameid/${target.gameId}`]);
  } catch (error) {
    rmSync(path.join(dir, REQUEST), { force: true });
    log(`Steam could not be asked: ${error.message}`);
    return false;
  }
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (existsSync(marker)) {
      rmSync(marker, { force: true });
      rmSync(path.join(dir, FAILED), { force: true });
      log("Steam started the game");
      return "steam";
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  rmSync(path.join(dir, REQUEST), { force: true });
  // Not again for a while, nor in the versions this one starts.
  try { writeFileSync(path.join(dir, FAILED), new Date().toISOString()); } catch {}
  env.BFE_NO_STEAM = "1";
  log("Steam did not start the game in time; starting directly");
  return false;
}

// The desktop settings' steam record (main.js keeps it in
// desktop-settings.json), read before the app has loaded.
export function readSteamSettings(userData) {
  try {
    const steam = JSON.parse(readFileSync(path.join(userData, "desktop-settings.json"), "utf8"))?.steam;
    return steam && typeof steam === "object" ? steam : null;
  } catch {
    return null;
  }
}
