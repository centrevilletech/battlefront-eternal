// The game in the player's Steam library, at the player's request
// (docs/presence.md): a non-Steam game entry that starts the game the way
// the desktop launcher does, with our own artwork, so Steam shows friends
// "playing Battlefront Eternal" whenever the game runs.
//
// Steam keeps its library in memory and writes shortcuts.vdf back when it
// changes, so the file is only ever written while Steam is closed: with
// Steam running, adding or removing the entry closes Steam first and
// starts it again afterwards, when the player agrees to that. The
// previous file is kept beside it (shortcuts.vdf.bfe-backup) the first
// time, and every write is a complete file renamed into place.
import { execFile } from "node:child_process";
import { mkdir, readFile, rename, rm, stat, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { FLATPAK_ID, gridDir, runningSteam, runSteam, shortcutsFile, steamAccounts, steamCommand, steamInstalls } from "./client.js";
import { gameLauncher, shortcutLaunch } from "./launcher.js";
import { describeShortcut, findShortcut, removeShortcut, shortcutAppId, upsertShortcut } from "./shortcut.js";
import { parseShortcuts } from "./vdf.js";

// The library's pictures (electron/assets/steam, tools/steam-artwork.mjs),
// named in config/grid for the entry's app id: the capsule, the portrait,
// the hero banner, the logo over it and the icon. Steam shows a custom
// logo only with its position (<app id>.json).
export const ARTWORK = [["capsule.jpg", ""], ["portrait.jpg", "p"], ["hero.jpg", "_hero"], ["logo.png", "_logo"], ["icon.png", "_icon"]];
export const LOGO_POSITION = { nVersion: 1, logoPosition: { pinnedPosition: "BottomLeft", nWidthPct: 50, nHeightPct: 50 } };
const gridName = (appid, [source, suffix]) => `${appid}${suffix}${path.extname(source)}`;
const PICTURE_TYPES = [".png", ".jpg", ".jpeg"];

async function readOptional(file) {
  try { return await readFile(file); } catch (error) { if (error.code === "ENOENT") return null; throw error; }
}

// A complete new file in place of the old, the first original kept.
export async function writeShortcuts(file, buffer, original) {
  await mkdir(path.dirname(file), { recursive: true });
  const backup = `${file}.bfe-backup`;
  if (original && !(await stat(backup).catch(() => null))) await writeFile(backup, original);
  const temp = `${file}.${process.pid}.tmp`;
  await writeFile(temp, buffer);
  await rename(temp, file);
}

// Whether a Flatpak Steam may ask the host to start programs
// (flatpak-spawn --host talks to org.freedesktop.Flatpak).
export function flatpakHostAccess({ exec = execFile } = {}) {
  return new Promise((resolve) => {
    exec("flatpak", ["info", "--show-permissions", FLATPAK_ID], { timeout: 5000 }, (error, stdout) => {
      if (error) return resolve(false);
      resolve(/^\s*org\.freedesktop\.Flatpak\s*=\s*talk\s*$/m.test(String(stdout)));
    });
  });
}

// Grants it (allow) or takes it back (the player removed the game, and the
// permission was this app's doing).
export function setFlatpakHostAccess(allow, { exec = execFile } = {}) {
  return new Promise((resolve, reject) => {
    exec("flatpak", ["override", "--user", `--${allow ? "" : "no-"}talk-name=org.freedesktop.Flatpak`, FLATPAK_ID], { timeout: 10000 }, (error) => (error ? reject(new Error("Flatpak refused the permission change.")) : resolve(true)));
  });
}

const until = async (check, { timeoutMs, every = 500 }) => {
  const end = Date.now() + timeoutMs;
  for (;;) {
    if (await check()) return true;
    if (Date.now() > end) return false;
    await new Promise((resolve) => setTimeout(resolve, every));
  }
};

// settings(): the desktop settings' steam record; save(record) stores it.
// artwork: the folder of ARTWORK pictures. bySteam(): whether Steam started
// this run (Steam cannot be closed under it).
export function createSteamIntegration({ settings, save, artwork, platform = process.platform, home = os.homedir(), env = process.env, execPath = process.execPath, packaged = true, bySteam = () => false,
  installs = steamInstalls, accounts = steamAccounts, running = runningSteam, command = steamCommand, run = runSteam, hostAccess = flatpakHostAccess, setHostAccess = setFlatpakHostAccess, log = () => {}, closeTimeoutMs = 45000 }) {
  let busy = null;
  const launchFor = (kind) => shortcutLaunch(gameLauncher({ platform, env, execPath, home, packaged }), kind, { home });

  // The installation and account the entry goes to: the running, signed-in
  // Steam's; else the one signed in most recently.
  async function target() {
    const found = await installs({ platform, home, env });
    if (!found.length) return null;
    const now = await running({ platform, home });
    const candidates = [];
    for (const install of found) {
      if (now.running && now.kind !== install.kind) continue;
      for (const account of await accounts(install.root)) candidates.push({ ...install, ...account, active: now.running && account.account === now.account });
    }
    candidates.sort((a, b) => b.active - a.active || b.mostRecent - a.mostRecent || b.time - a.time);
    const chosen = candidates[0];
    return chosen ? { ...chosen, running: !!now.running, signedIn: !!now.account, game: now.game || 0 } : null;
  }

  async function entryIn(found, appid) {
    const list = parseShortcuts(await readOptional(shortcutsFile(found.root, found.account)));
    const index = findShortcut(list.entries, { appid });
    return index < 0 ? null : describeShortcut(list.entries[index]);
  }

  // What the options page shows.
  async function state() {
    const record = settings() ?? {};
    let found;
    try { found = await target(); } catch (error) { return { available: false, problem: error.message }; }
    if (!found) return { available: false, problem: platform === "darwin" ? "unsupported" : "no-steam" };
    let entry = null, problem = null;
    try { entry = await entryIn(found, record.appid ?? null); } catch { problem = "unreadable"; }
    const launch = launchFor(found.kind);
    if (!launch) problem ??= found.kind === "snap" ? "snap" : "no-launcher";
    const needsAccess = found.kind === "flatpak" && !(await hostAccess());
    return {
      available: true,
      kind: found.kind,
      account: found.account,
      name: found.name || null,
      running: found.running,
      added: !!entry,
      launch: record.launch !== false,
      needsAccess,
      problem,
    };
  }

  // Steam closed, then `work`, then Steam started again if it was running.
  // In a gamescope session (Steam Deck's Game Mode) Steam is the session:
  // closing it would end it.
  async function withSteamClosed(found, { restart }, work) {
    if (!found.running) return work();
    if (/gamescope/i.test(env.XDG_CURRENT_DESKTOP ?? "") || env.GAMESCOPE_WAYLAND_DISPLAY) throw new Error("Steam runs the session here. Change its library from the desktop instead.");
    // Closing Steam would close the game it runs (this one, or another).
    if (bySteam()) throw new Error("Steam started this game, so it cannot close now. Quit, close Steam, then start Battlefront Eternal and change it here.");
    if (found.game) throw new Error("A game is running in Steam. Close it first.");
    if (!restart) return { needsRestart: true };
    const steam = await command({ kind: found.kind, root: found.root, platform, env });
    log("closing Steam to change its library");
    run(steam, ["-shutdown"]);
    if (!(await until(async () => !(await running({ platform, home })).running, { timeoutMs: closeTimeoutMs }))) {
      throw new Error("Steam did not close. Close Steam yourself, then try again.");
    }
    try {
      return await work();
    } finally {
      log("starting Steam again");
      run(steam, ["-silent"]);
    }
  }

  async function placeArtwork(found, appid) {
    if (!artwork) return;
    const grid = gridDir(found.root, found.account);
    await mkdir(grid, { recursive: true });
    const exists = async (file) => !!(await stat(file).catch(() => null));
    for (const picture of ARTWORK) {
      const [source, suffix] = picture;
      // The player's own artwork for that place stays, whatever its type
      // (Steam would prefer a JPEG over their PNG).
      let own = false;
      for (const type of PICTURE_TYPES) own ||= await exists(path.join(grid, `${appid}${suffix}${type}`));
      if (own) continue;
      // Read, not copied: the pictures are inside the app's archive.
      await readFile(path.join(artwork, source)).then((bytes) => writeFile(path.join(grid, gridName(appid, picture)), bytes)).catch((error) => log(`artwork ${source}: ${error.message}`));
    }
    const position = path.join(grid, `${appid}.json`);
    if (!(await exists(position))) await writeFile(position, JSON.stringify(LOGO_POSITION)).catch((error) => log(`logo position: ${error.message}`));
  }

  // Our pictures for an entry being removed: only those still exactly as
  // written (the player may have replaced them).
  async function removeArtwork(found, appid) {
    if (!artwork) return;
    const grid = gridDir(found.root, found.account);
    const same = async (file, bytes) => { try { return (await readFile(file)).equals(bytes); } catch { return false; } };
    for (const picture of ARTWORK) {
      const file = path.join(grid, gridName(appid, picture));
      if (await same(file, await readFile(path.join(artwork, picture[0])).catch(() => Buffer.alloc(0)))) await rm(file, { force: true });
    }
    const position = path.join(grid, `${appid}.json`);
    if (await same(position, Buffer.from(JSON.stringify(LOGO_POSITION)))) await rm(position, { force: true });
  }

  // `grant`: the player agreed to the Flatpak permission (main.js asks in
  // a dialog of its own, never on a page's word).
  async function add({ restart = false, grant = false } = {}) {
    const found = await target();
    if (!found) throw new Error("Steam is not installed for this user.");
    const launch = launchFor(found.kind);
    if (!launch) throw new Error(found.kind === "snap" ? "The Snap version of Steam cannot start programs installed outside it." : "This copy of the game cannot be started from Steam.");
    let granted = false;
    if (found.kind === "flatpak" && !(await hostAccess())) {
      if (!grant) return { needsAccess: true };
      await setHostAccess(true);
      granted = true;
    }
    const record = settings() ?? {};
    return withSteamClosed(found, { restart }, async () => {
      const file = shortcutsFile(found.root, found.account);
      const original = await readOptional(file);
      // The entry already there keeps its app id; a new one gets Steam's.
      const known = record.root === found.root && record.account === found.account ? record.appid ?? null : null;
      const list = parseShortcuts(original);
      const index = findShortcut(list.entries, { appid: known });
      const appid = index >= 0 ? describeShortcut(list.entries[index]).appid : shortcutAppId(launch.exe);
      const icon = artwork ? path.join(gridDir(found.root, found.account), `${appid}_icon.png`) : "";
      const result = upsertShortcut(original, { appid, ...launch, icon });
      if (result.changed) await writeShortcuts(file, result.buffer, original);
      await placeArtwork(found, result.appid);
      // What it was written with, so a later start can tell the player's
      // changes from a launcher that moved (maintain).
      await save({ appid: result.appid, root: found.root, kind: found.kind, account: found.account, launch: record.launch !== false, overlay: record.overlay === true, ...writtenWith(file, result), grantedAccess: granted || !!record.grantedAccess });
      log(`${result.added ? "added to" : "updated in"} Steam (${found.kind}, account ${found.account}, app ${result.appid})`);
      return { added: true, appid: result.appid };
    });
  }

  async function remove({ restart = false } = {}) {
    const record = settings() ?? {};
    const found = await target();
    if (!found) {
      await save({ ...record, appid: null });
      return { removed: false };
    }
    return withSteamClosed(found, { restart }, async () => {
      const file = shortcutsFile(found.root, found.account);
      const original = await readOptional(file);
      const list = parseShortcuts(original);
      const index = findShortcut(list.entries, { appid: record.appid ?? null });
      const appid = index >= 0 ? describeShortcut(list.entries[index]).appid : null;
      const result = original && appid !== null ? removeShortcut(original, { appid }) : { removed: false };
      if (result.removed) {
        await writeShortcuts(file, result.buffer, original);
        await removeArtwork(found, appid);
      }
      // The permission this app asked a Flatpak Steam to have goes with it.
      if (record.grantedAccess && found.kind === "flatpak") await setHostAccess(false).catch((error) => log(`Flatpak permission: ${error.message}`));
      await save({ launch: record.launch !== false, overlay: record.overlay === true, declined: true });
      log(`removed from Steam (${found.kind}, account ${found.account})`);
      return { removed: result.removed };
    });
  }

  // At startup with Steam closed: an entry whose launcher moved (an
  // AppImage moved, a package replaced) is pointed at the current one; an
  // entry the player deleted in Steam stays deleted.
  // How the entry starts, as written: { exe, startDir, launchOptions }.
  function writtenWith(file, result) {
    const list = parseShortcuts(result.buffer);
    const entry = describeShortcut(list.entries[findShortcut(list.entries, { appid: result.appid })]);
    return { exe: entry.exe, startDir: entry.startDir, launchOptions: entry.launchOptions };
  }

  async function maintain() {
    const record = settings();
    if (!record?.appid || !record.root || !record.account) return null;
    const now = await running({ platform, home });
    if (now.running) return null;
    const launch = launchFor(record.kind);
    if (!launch) return null;
    const file = shortcutsFile(record.root, record.account);
    const original = await readOptional(file);
    if (!original) return null;
    const list = parseShortcuts(original);
    const index = findShortcut(list.entries, { appid: record.appid });
    if (index < 0) {
      await save({ launch: record.launch !== false, overlay: record.overlay === true, declined: true, grantedAccess: !!record.grantedAccess });
      log("the Steam library entry was removed in Steam; it stays removed");
      return "removed";
    }
    const entry = describeShortcut(list.entries[index]);
    if (entry.exe === launch.exe && entry.startDir === launch.startDir && entry.launchOptions.includes(launch.launchOptions)) return "current";
    // The player pointed it elsewhere (a wrapper of their own): theirs to
    // keep. A Flatpak entry names the launcher in its options, so there
    // the options must be as written too.
    const flatpak = record.kind === "flatpak";
    if (entry.exe !== record.exe || entry.startDir !== record.startDir || (flatpak && entry.launchOptions !== record.launchOptions)) return "theirs";
    // The launcher moved: follow it, with the player's own options where
    // they still carry ours (`gamemoderun %command% --steam`).
    const launchOptions = !flatpak && entry.launchOptions.includes(launch.launchOptions) ? entry.launchOptions : launch.launchOptions;
    const result = upsertShortcut(original, { appid: record.appid, ...launch, launchOptions });
    if (!result.changed) return "current";
    await writeShortcuts(file, result.buffer, original);
    await save({ ...record, ...writtenWith(file, result) });
    log(`pointed the Steam library entry at ${launch.exe}`);
    return "updated";
  }

  const once = (work) => (...args) => {
    if (busy) return Promise.reject(new Error("Steam is already being changed."));
    busy = work(...args).finally(() => { busy = null; });
    return busy;
  };

  return { state, add: once(add), remove: once(remove), maintain: once(maintain), target };
}
