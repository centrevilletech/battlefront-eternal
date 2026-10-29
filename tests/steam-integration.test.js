import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { after, test } from "node:test";
import { linuxSteamProcesses, parseRegQuery, rootKind, runningSteam, steamAccounts, steamCommand, steamEnvironment } from "../electron/steam/client.js";
import { ARTWORK, createSteamIntegration, LOGO_POSITION } from "../electron/steam/index.js";
import { canRestartSelf, gameLauncher, shortcutLaunch, startedBySteam, steamLost, steamStays, withoutSteamOverlay } from "../electron/steam/launcher.js";
import { arrived, readSteamSettings, relaunchThroughSteam, steamEntry } from "../electron/steam/relaunch.js";
import { describeShortcut, findShortcut, STEAM_ARG } from "../electron/steam/shortcut.js";
import { parseShortcuts, serializeShortcuts } from "../electron/steam/vdf.js";

const linux = process.platform === "linux";
const temp = mkdtempSync(path.join(tmpdir(), "bfe-steam-"));
after(() => rmSync(temp, { recursive: true, force: true }));
let count = 0;
const scratch = (name) => {
  const dir = path.join(temp, `${name}-${++count}`);
  mkdirSync(dir, { recursive: true });
  return dir;
};
const touch = (file, text = "", mode) => {
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, text, mode ? { mode } : undefined);
};

const LOGINUSERS = (accounts) => `"users"\n{\n${accounts.map(([id, name, recent, time]) => `\t"${76561197960265728n + BigInt(id)}"\n\t{\n\t\t"AccountName"\t\t"${name}"\n\t\t"PersonaName"\t\t"${name.toUpperCase()}"\n\t\t"MostRecent"\t\t"${recent ? 1 : 0}"\n\t\t"Timestamp"\t\t"${time}"\n\t}\n`).join("")}}\n`;

function steamRoot(dir, accounts) {
  touch(path.join(dir, "config", "loginusers.vdf"), LOGINUSERS(accounts));
  for (const [id] of accounts) mkdirSync(path.join(dir, "userdata", String(id), "config"), { recursive: true });
  mkdirSync(path.join(dir, "steamapps"), { recursive: true });
  return dir;
}

test("Steam's installations by kind, and their accounts, most recent first", async () => {
  assert.equal(rootKind("/home/j/.local/share/Steam", "linux"), "native");
  assert.equal(rootKind("/home/j/.var/app/com.valvesoftware.Steam/.local/share/Steam", "linux"), "flatpak");
  assert.equal(rootKind("/home/j/snap/steam/common/.local/share/Steam", "linux"), "snap");
  assert.equal(rootKind("C:\\Program Files (x86)\\Steam", "win32"), "windows");
  const root = steamRoot(scratch("root"), [[11, "older", false, 100], [22, "recent", true, 50]]);
  mkdirSync(path.join(root, "userdata", "0"), { recursive: true });
  mkdirSync(path.join(root, "userdata", "33"), { recursive: true });
  const accounts = await steamAccounts(root);
  assert.deepEqual(accounts.map((a) => a.account), ["22", "11", "33"]);
  assert.equal(accounts[0].name, "RECENT");
});

test("the running client is found in /proc, its kind from its environment, its account from registry.vdf", { skip: !linux && "symlinks" }, async () => {
  const proc = scratch("proc"), home = scratch("home");
  const fake = (pid, exe, environ = "") => {
    mkdirSync(path.join(proc, String(pid)), { recursive: true });
    symlinkSync(exe, path.join(proc, String(pid), "exe"));
    writeFileSync(path.join(proc, String(pid), "environ"), environ);
  };
  fake(100, "/usr/bin/bash");
  fake(200, "/home/j/.local/share/Steam/ubuntu12_64/steamwebhelper");
  assert.deepEqual(await linuxSteamProcesses({ proc }), []);
  assert.deepEqual(await runningSteam({ platform: "linux", home, proc }), { running: false });
  fake(300, "/home/j/.var/app/com.valvesoftware.Steam/.local/share/Steam/ubuntu12_32/steam", "HOME=/home/j\0FLATPAK_ID=com.valvesoftware.Steam\0");
  touch(path.join(home, ".var", "app", "com.valvesoftware.Steam", ".steam", "registry.vdf"), `"Registry" { "HKCU" { "Software" { "Valve" { "Steam" { "RunningAppID" "1058020" "ActiveProcess" { "pid" "12" "ActiveUser" "22" } } } } } }`);
  assert.deepEqual(await runningSteam({ platform: "linux", home, proc }), { running: true, kind: "flatpak", pid: 300, account: "22", game: 1058020 });
  // Signed out: running, no account.
  touch(path.join(home, ".var", "app", "com.valvesoftware.Steam", ".steam", "registry.vdf"), `"Registry" { "HKCU" { "Software" { "Valve" { "Steam" { "ActiveProcess" { "pid" "12" "ActiveUser" "0" } } } } } }`);
  assert.equal((await runningSteam({ platform: "linux", home, proc })).account, null);
});

test("Windows: reg query's output as Windows prints it", () => {
  const active = "\r\nHKEY_CURRENT_USER\\Software\\Valve\\Steam\\ActiveProcess\r\n    pid    REG_DWORD    0x3e8\r\n    SteamClientDll    REG_SZ    C:\\Program Files (x86)\\Steam\\steamclient.dll\r\n    SteamClientDll64    REG_SZ    C:\\Program Files (x86)\\Steam\\steamclient64.dll\r\n    Universe    REG_SZ    Public\r\n    ActiveUser    REG_DWORD    0x25e5f01\r\n\r\n";
  assert.deepEqual(parseRegQuery(active), { pid: 1000, SteamClientDll: "C:\\Program Files (x86)\\Steam\\steamclient.dll", SteamClientDll64: "C:\\Program Files (x86)\\Steam\\steamclient64.dll", Universe: "Public", ActiveUser: 0x25e5f01 });
  const steam = "\r\nHKEY_CURRENT_USER\\Software\\Valve\\Steam\r\n    SteamExe    REG_SZ    c:/program files (x86)/steam/steam.exe\r\n    SteamPath    REG_SZ    c:/program files (x86)/steam\r\n    RunningAppID    REG_DWORD    0x0\r\n";
  assert.deepEqual(parseRegQuery(steam), { SteamExe: "c:/program files (x86)/steam/steam.exe", SteamPath: "c:/program files (x86)/steam", RunningAppID: 0 });
});

test("Windows: the registry's active process, if it still runs, and its signed-in account", async () => {
  const registry = (values) => async () => values;
  assert.deepEqual(await runningSteam({ platform: "win32", registry: registry({ pid: 0, ActiveUser: 0 }) }), { running: false });
  assert.deepEqual(await runningSteam({ platform: "win32", registry: registry({ pid: process.pid, ActiveUser: 22, RunningAppID: 0 }) }), { running: true, kind: "windows", pid: process.pid, account: "22", game: 0 });
  assert.deepEqual(await runningSteam({ platform: "win32", registry: registry({ pid: 2 ** 22 + 17, ActiveUser: 22 }) }), { running: false }, "a stale pid");
  const command = await steamCommand({ platform: "win32", root: "C:\\Steam", registry: registry({ SteamExe: "c:/program files (x86)/steam/steam.exe" }) });
  assert.equal(command.file, path.normalize("c:/program files (x86)/steam/steam.exe"));
  assert.deepEqual(await steamCommand({ platform: "linux", kind: "flatpak" }), { file: "flatpak", args: ["run", "com.valvesoftware.Steam"] });
});

test("Steam started from the game gets the session's environment, not the game's", () => {
  const env = steamEnvironment({ HOME: "/home/j", PATH: "/tmp/.mount_x/usr/bin:/usr/bin", APPDIR: "/tmp/.mount_x", APPIMAGE: "/home/j/BFE.AppImage", BFE_HANDOFF: "1", BFE_BOOTSTRAP_DIR: "/opt/x",
    ELECTRON_RUN_AS_NODE: "1", CHROME_DEVEL_SANDBOX: "/opt/x/chrome-sandbox", NO_AT_BRIDGE: "1", FC_FONTATIONS: "1", DISPLAY: ":0", WAYLAND_DISPLAY: "wayland-1", HYPRLAND_INSTANCE_SIGNATURE: "abc",
    // What Steam's launch of this game added.
    SteamGameId: "123", SteamAppId: "0", SteamOverlayGameId: "123", SteamClientLaunch: "1", SteamEnv: "1", SteamNoOverlayUI: "1", EnableConfiguratorSupport: "4111",
    SDL_GAMECONTROLLER_IGNORE_DEVICES: "0x28de/0x11ff", STEAM_COMPAT_MEDIA_PATH: "/x", STEAM_RUNTIME: "/s/ubuntu12_32/steam-runtime", STEAM_RUNTIME_LIBRARY_PATH: "/s", ENABLE_VK_LAYER_VALVE_steam_overlay_1: "1",
    LD_PRELOAD: ":/s/ubuntu12_32/gameoverlayrenderer.so:/usr/lib/libgamemodeauto.so.0", LD_LIBRARY_PATH: "/s/ubuntu12_32/steam-runtime/usr/lib:/opt/cuda/lib64" }, "linux");
  assert.deepEqual(env, { HOME: "/home/j", PATH: "/usr/bin", DISPLAY: ":0", WAYLAND_DISPLAY: "wayland-1", HYPRLAND_INSTANCE_SIGNATURE: "abc", LD_PRELOAD: "/usr/lib/libgamemodeauto.so.0", LD_LIBRARY_PATH: "/opt/cuda/lib64" });
});

test("the entry starts the game the way the player's launcher does", () => {
  const home = scratch("home");
  const data = path.join(home, ".local", "share", "battlefront-eternal");
  const version = path.join(data, "versions", "0.1.0-preview.300", "battlefront-eternal");
  // Debian: the package's executable, also when an update runs.
  assert.equal(gameLauncher({ platform: "linux", home, env: { BFE_BOOTSTRAP_DIR: "/opt/Battlefront Eternal" }, execPath: version }), null, "not installed here");
  const deb = path.join(home, "opt", "Battlefront Eternal");
  touch(path.join(deb, "battlefront-eternal"), "", 0o755);
  assert.deepEqual(gameLauncher({ platform: "linux", home, env: { BFE_BOOTSTRAP_DIR: deb }, execPath: version }), { file: path.join(deb, "battlefront-eternal"), startDir: deb });
  // install.sh's AppImage: its launcher in ~/.local/bin.
  touch(path.join(home, ".local", "bin", "battlefront-eternal"), "#!/usr/bin/env bash\n# Created by the Battlefront Eternal installer. Run it again to update.\nexec x\n", 0o755);
  assert.deepEqual(gameLauncher({ platform: "linux", home, env: {}, execPath: version }), { file: path.join(home, ".local", "bin", "battlefront-eternal"), startDir: home });
  assert.deepEqual(gameLauncher({ platform: "linux", home, env: {}, execPath: path.join(data, "app", "battlefront-eternal") }).file, path.join(home, ".local", "bin", "battlefront-eternal"));
  // A launcher someone else wrote is not ours.
  touch(path.join(home, ".local", "bin", "battlefront-eternal"), "#!/bin/sh\nexec something-else\n", 0o755);
  assert.equal(gameLauncher({ platform: "linux", home, env: {}, execPath: version }), null);
  const appImage = path.join(home, "Games", "Battlefront-Eternal.AppImage");
  touch(appImage, "", 0o755);
  assert.equal(gameLauncher({ platform: "linux", home, env: { APPIMAGE: appImage }, execPath: "/tmp/.mount_x/battlefront-eternal" }).file, appImage);
  touch(path.join(home, ".local", "bin", "battlefront"), "#!/bin/sh\n# Created by Battlefront Eternal; it follows the AppImage when it moves.\n", 0o755);
  assert.equal(gameLauncher({ platform: "linux", home, env: { BFE_APPIMAGE: appImage }, execPath: version }).file, path.join(home, ".local", "bin", "battlefront"));
  // Development runs have no entry.
  assert.equal(gameLauncher({ platform: "linux", home, env: {}, execPath: version, packaged: false }), null);
  // Windows: the installed executable, never an update's own.
  const local = path.join(home, "AppData", "Local");
  const installed = path.join(local, "Programs", "battlefront-eternal", "Battlefront Eternal.exe");
  touch(installed);
  const update = path.join(local, "Battlefront Eternal", "versions", "0.1.0-preview.300", "Battlefront Eternal.exe");
  assert.equal(gameLauncher({ platform: "win32", home, env: { LOCALAPPDATA: local }, execPath: installed }).file, installed);
  assert.equal(gameLauncher({ platform: "win32", home, env: { LOCALAPPDATA: local, BFE_BOOTSTRAP_DIR: path.dirname(installed) }, execPath: update }).file, installed);
  assert.equal(gameLauncher({ platform: "win32", home, env: { LOCALAPPDATA: local }, execPath: update }), null);
});

test("each Steam kind starts it its own way; Snap cannot", () => {
  const launcher = { file: "/home/j/.local/bin/battlefront-eternal", startDir: "/home/j" };
  assert.deepEqual(shortcutLaunch(launcher, "native"), { exe: '"/home/j/.local/bin/battlefront-eternal"', startDir: '"/home/j"', launchOptions: STEAM_ARG });
  assert.deepEqual(shortcutLaunch(launcher, "flatpak", { home: "/home/j" }), { exe: '"/usr/bin/flatpak-spawn"', startDir: '"/home/j"', launchOptions: `--host --watch-bus "/home/j/.local/bin/battlefront-eternal" ${STEAM_ARG}` });
  // The package's folder is not in Flatpak's sandbox: the home folder is.
  assert.equal(shortcutLaunch({ file: "/opt/Battlefront Eternal/battlefront-eternal", startDir: "/opt/Battlefront Eternal" }, "flatpak", { home: "/home/j" }).startDir, '"/home/j"');
  assert.equal(shortcutLaunch(launcher, "snap"), null);
  assert.equal(shortcutLaunch(null, "native"), null);
  assert.equal(shortcutLaunch({ file: "C:\\Games\\BFE\\Battlefront Eternal.exe", startDir: "C:\\Games\\BFE" }, "windows").startDir, path.sep === "\\" ? '"C:\\Games\\BFE\\"' : '"C:\\Games\\BFE/"');
  assert.equal(startedBySteam({ argv: ["exe", STEAM_ARG], env: {} }), true);
  assert.equal(startedBySteam({ argv: ["exe"], env: { SteamGameId: "13030803944476606464" } }), true);
  assert.equal(startedBySteam({ argv: ["exe"], env: { SteamAppId: "0" } }), false);
  assert.equal(startedBySteam({ argv: ["exe"], env: {} }), false);
});

test("a start outside Steam goes through it: the copy Steam starts answers, and this one exits", async () => {
  const dir = scratch("launches");
  const target = { steam: { kind: "native" }, command: { file: "steam", args: [] }, gameId: "123" };
  const asked = [];
  // Steam starts the entry a little later; that copy finds the request.
  const run = (command, args) => {
    asked.push(args);
    setTimeout(() => assert.equal(arrived({ dir, argv: ["exe", STEAM_ARG], env: {} }), true), 150);
  };
  const logs = [];
  assert.equal(await relaunchThroughSteam({ dir, steam: { appid: 1 }, argv: ["exe"], env: {}, find: async () => target, run, log: (l) => logs.push(l) }), "steam");
  assert.deepEqual(asked, [["steam://rungameid/123"]]);
  assert.ok(!existsSync(path.join(dir, "steam-relaunch.json")));
  // Started by Steam, or with nothing in Steam, or told not to: no round trip.
  assert.equal(await relaunchThroughSteam({ dir, steam: { appid: 1 }, argv: ["exe", STEAM_ARG], env: {}, find: async () => target, run }), false);
  assert.equal(await relaunchThroughSteam({ dir, steam: { appid: 1 }, argv: ["exe"], env: {}, find: async () => null, run }), false);
  assert.equal(await relaunchThroughSteam({ dir, steam: { appid: 1 }, argv: ["exe"], env: { BFE_NO_STEAM: "1" }, find: async () => target, run }), false);
  assert.equal(asked.length, 1);
});

test("when Steam does not start the game in time, this copy carries on, and neither it nor the next starts ask again for a while", async () => {
  const dir = scratch("launches");
  const target = { steam: { kind: "native" }, command: { file: "steam", args: [] }, gameId: "123" };
  const logs = [];
  const begun = Date.now();
  const env = {};
  assert.equal(await relaunchThroughSteam({ dir, steam: { appid: 1 }, argv: ["exe"], env, timeoutMs: 300, find: async () => target, run: () => {}, log: (l) => logs.push(l) }), false);
  assert.ok(Date.now() - begun < 2000);
  assert.ok(logs.some((l) => /did not start/.test(l)));
  assert.ok(!existsSync(path.join(dir, "steam-relaunch.json")), "the request is withdrawn");
  assert.equal(env.BFE_NO_STEAM, "1", "the versions this one hands off to carry on too");
  let asked = 0;
  assert.equal(await relaunchThroughSteam({ dir, steam: { appid: 1 }, argv: ["exe"], env: {}, timeoutMs: 300, find: async () => target, run: () => { asked++; } }), false);
  assert.equal(asked, 0, "the next start goes directly");
  // A success clears it.
  rmSync(path.join(dir, "steam-relaunch-failed"));
  const answer = () => setTimeout(() => arrived({ dir, argv: ["exe", STEAM_ARG], env: {} }), 100);
  assert.equal(await relaunchThroughSteam({ dir, steam: { appid: 1 }, argv: ["exe"], env: {}, find: async () => target, run: () => { asked++; answer(); } }), "steam");
  assert.equal(asked, 1);
});

test("a second click while a start goes through Steam waits for it; a request left by a dead start does not", async () => {
  const dir = scratch("launches");
  const target = { steam: { kind: "native" }, command: { file: "steam", args: [] }, gameId: "123" };
  // A start still waiting (this test's own parent stands in for it).
  writeFileSync(path.join(dir, "steam-relaunch.json"), JSON.stringify({ token: "a".repeat(32), pid: process.ppid }));
  let asked = 0;
  assert.equal(await relaunchThroughSteam({ dir, steam: { appid: 1 }, argv: ["exe"], env: {}, find: async () => target, run: () => { asked++; } }), "waiting");
  assert.equal(asked, 0);
  // Its start is gone: a fresh round trip.
  writeFileSync(path.join(dir, "steam-relaunch.json"), JSON.stringify({ token: "a".repeat(32), pid: 2 ** 22 + 17 }));
  assert.equal(await relaunchThroughSteam({ dir, steam: { appid: 1 }, argv: ["exe"], env: {}, timeoutMs: 200, find: async () => target, run: () => { asked++; } }), false);
  assert.equal(asked, 1);
  // A copy Steam started answers only a fresh request.
  writeFileSync(path.join(dir, "steam-relaunch.json"), JSON.stringify({ token: "b".repeat(32), pid: process.ppid }));
  assert.equal(arrived({ dir, argv: ["exe"], env: {} }), false, "not started by Steam");
  assert.equal(arrived({ dir, argv: ["exe", STEAM_ARG], env: {} }), true);
  assert.equal(readFileSync(path.join(dir, "b".repeat(32)), "utf8"), String(process.pid));
});

test("the entry Steam would start: the running, signed-in account's, if it is there and its launcher exists", { skip: !linux && "paths" }, async () => {
  const root = steamRoot(scratch("steam"), [[22, "j", true, 1]]);
  const launcher = path.join(scratch("bin"), "battlefront-eternal");
  touch(launcher, "#!/bin/sh\n", 0o755);
  const steamIntegration = createSteamIntegration({ settings: () => record, save: async (next) => { record = next; }, artwork: null, platform: "linux", home: scratch("home"), env: { BFE_STEAM_LAUNCHER: launcher },
    installs: async () => [{ root, kind: "native" }], running: async () => ({ running: false }), run: () => {}, hostAccess: async () => true });
  let record = null;
  await steamIntegration.add();
  const command = async () => ({ file: "steam", args: [] });
  const running = (account) => async () => ({ running: true, kind: "native", pid: 1, account });
  const found = await steamEntry({ steam: record, platform: "linux", running: running("22"), command });
  assert.equal(found.gameId, String((BigInt(record.appid) << 32n) | 0x02000000n));
  assert.equal(await steamEntry({ steam: record, platform: "linux", running: running(null), command }), null, "signed out");
  assert.equal(await steamEntry({ steam: record, platform: "linux", running: running("99"), command }), null, "another account");
  assert.equal(await steamEntry({ steam: { ...record, launch: false }, platform: "linux", running: running("22"), command }), null, "the player turned it off");
  rmSync(launcher);
  assert.equal(await steamEntry({ steam: record, platform: "linux", running: running("22"), command }), null, "its launcher is gone");
});

test("adding and removing: written only with Steam closed, artwork placed, the original kept", { skip: !linux && "paths" }, async () => {
  const root = steamRoot(scratch("steam"), [[22, "j", true, 1]]);
  const artwork = scratch("art");
  for (const [name] of ARTWORK) touch(path.join(artwork, name), name);
  const launcher = path.join(scratch("bin"), "battlefront-eternal");
  touch(launcher, "#!/bin/sh\n", 0o755);
  const original = Buffer.from("\0shortcuts\0\b\b", "latin1");
  touch(path.join(root, "userdata", "22", "config", "shortcuts.vdf"), original);
  let record = null, steamUp = true;
  const ran = [];
  const steam = createSteamIntegration({ settings: () => record, save: async (next) => { record = next; }, artwork, platform: "linux", home: scratch("home"), env: { BFE_STEAM_LAUNCHER: launcher }, closeTimeoutMs: 3000,
    installs: async () => [{ root, kind: "native" }], running: async () => (steamUp ? { running: true, kind: "native", pid: 1, account: "22" } : { running: false }),
    command: async () => ({ file: "steam", args: [] }), run: (_command, args) => { ran.push(args[0]); if (args[0] === "-shutdown") setTimeout(() => { steamUp = false; }, 200); if (args[0] === "-silent") steamUp = true; } });
  const before = await steam.state();
  assert.deepEqual({ ...before }, { available: true, kind: "native", account: "22", name: "J", running: true, added: false, launch: true, needsAccess: false, problem: null });
  // Steam is open: nothing is written without the player's yes to a restart.
  assert.deepEqual(await steam.add(), { needsRestart: true });
  assert.ok(readFileSync(path.join(root, "userdata", "22", "config", "shortcuts.vdf")).equals(original));
  assert.deepEqual(await steam.add({ restart: true }), { added: true, appid: record.appid });
  assert.deepEqual(ran, ["-shutdown", "-silent"]);
  const file = path.join(root, "userdata", "22", "config", "shortcuts.vdf");
  const entry = describeShortcut(parseShortcuts(readFileSync(file)).entries[0]);
  assert.equal(entry.exe, `"${launcher}"`);
  assert.equal(entry.appid, record.appid);
  assert.ok(readFileSync(`${file}.bfe-backup`).equals(original));
  for (const [name, suffix] of ARTWORK) assert.equal(readFileSync(path.join(root, "userdata", "22", "config", "grid", `${record.appid}${suffix}${path.extname(name)}`), "utf8"), name);
  assert.deepEqual(JSON.parse(readFileSync(path.join(root, "userdata", "22", "config", "grid", `${record.appid}.json`), "utf8")), LOGO_POSITION);
  assert.equal((await steam.state()).added, true);
  // The launcher moves; at the next start with Steam closed the entry follows.
  steamUp = false;
  const moved = path.join(scratch("bin"), "battlefront-eternal");
  touch(moved, "#!/bin/sh\n", 0o755);
  const later = createSteamIntegration({ settings: () => record, save: async (next) => { record = next; }, artwork, platform: "linux", home: scratch("home"), env: { BFE_STEAM_LAUNCHER: moved },
    installs: async () => [{ root, kind: "native" }], running: async () => ({ running: false }), run: () => {} });
  assert.equal(await later.maintain(), "updated");
  assert.equal(describeShortcut(parseShortcuts(readFileSync(file)).entries[0]).exe, `"${moved}"`);
  assert.equal(await later.maintain(), "current");
  // Removing it takes our pictures, not one the player put in its place.
  const grid = path.join(root, "userdata", "22", "config", "grid");
  writeFileSync(path.join(grid, `${entry.appid}_hero.jpg`), "the player's own");
  const removed = await later.remove();
  assert.deepEqual(removed, { removed: true });
  assert.equal(findShortcut(parseShortcuts(readFileSync(file)).entries, {}), -1);
  assert.equal(existsSync(path.join(grid, `${entry.appid}p.jpg`)), false);
  assert.equal(existsSync(path.join(grid, `${entry.appid}.json`)), false);
  assert.equal(readFileSync(path.join(grid, `${entry.appid}_hero.jpg`), "utf8"), "the player's own");
  assert.equal(record.declined, true);
  assert.equal(await later.maintain(), null);
});

test("the player's own artwork stays, whatever its type", { skip: !linux && "paths" }, async () => {
  const root = steamRoot(scratch("steam"), [[22, "j", true, 1]]);
  const artwork = scratch("art");
  for (const [name] of ARTWORK) touch(path.join(artwork, name), name);
  const launcher = path.join(scratch("bin"), "battlefront-eternal");
  touch(launcher, "#!/bin/sh\n", 0o755);
  let record = null;
  const steam = createSteamIntegration({ settings: () => record, save: async (next) => { record = next; }, artwork, platform: "linux", home: scratch("home"), env: { BFE_STEAM_LAUNCHER: launcher },
    installs: async () => [{ root, kind: "native" }], running: async () => ({ running: false }), run: () => {} });
  // Their portrait as a PNG for the id this entry will get.
  const { shortcutAppId } = await import("../electron/steam/shortcut.js");
  const appid = shortcutAppId(`"${launcher}"`);
  const grid = path.join(root, "userdata", "22", "config", "grid");
  touch(path.join(grid, `${appid}p.png`), "theirs");
  touch(path.join(grid, `${appid}.json`), '{"nVersion":1,"logoPosition":{"pinnedPosition":"UpperCenter","nWidthPct":40,"nHeightPct":40}}');
  await steam.add();
  assert.equal(record.appid, appid);
  assert.equal(existsSync(path.join(grid, `${appid}p.jpg`)), false, "no JPEG over their PNG");
  assert.match(readFileSync(path.join(grid, `${appid}.json`), "utf8"), /UpperCenter/);
  assert.equal(readFileSync(path.join(grid, `${appid}.jpg`), "utf8"), "capsule.jpg");
});

test("a Flatpak Steam needs the player's permission to start host programs; Snap cannot start the game", { skip: !linux && "paths" }, async () => {
  const root = steamRoot(scratch("steam"), [[22, "j", true, 1]]);
  const launcher = path.join(scratch("bin"), "battlefront-eternal");
  touch(launcher, "#!/bin/sh\n", 0o755);
  let record = null, granted = false;
  const flatpak = createSteamIntegration({ settings: () => record, save: async (next) => { record = next; }, artwork: null, platform: "linux", home: scratch("home"), env: { BFE_STEAM_LAUNCHER: launcher },
    installs: async () => [{ root, kind: "flatpak" }], running: async () => ({ running: false }), run: () => {}, hostAccess: async () => granted, setHostAccess: async (allow) => { granted = allow; } });
  assert.equal((await flatpak.state()).needsAccess, true);
  assert.deepEqual(await flatpak.add(), { needsAccess: true });
  assert.equal(granted, false, "never without the player's yes");
  assert.equal((await flatpak.add({ grant: true })).added, true);
  assert.equal(granted, true);
  assert.equal(record.grantedAccess, true);
  const entry = describeShortcut(parseShortcuts(readFileSync(path.join(root, "userdata", "22", "config", "shortcuts.vdf"))).entries[0]);
  assert.equal(entry.exe, '"/usr/bin/flatpak-spawn"');
  assert.match(entry.launchOptions, /^--host --watch-bus ".*battlefront-eternal" --steam$/);
  // Removed again: the permission this app asked for goes with it.
  await flatpak.remove();
  assert.equal(granted, false);
  const snap = createSteamIntegration({ settings: () => null, save: async () => {}, artwork: null, platform: "linux", home: scratch("home"), env: { BFE_STEAM_LAUNCHER: launcher },
    installs: async () => [{ root, kind: "snap" }], running: async () => ({ running: false }), run: () => {} });
  assert.equal((await snap.state()).problem, "snap");
  await assert.rejects(snap.add(), /Snap/);
});

test("a later start keeps the player's own changes to how the entry starts", { skip: !linux && "paths" }, async () => {
  const root = steamRoot(scratch("steam"), [[22, "j", true, 1]]);
  const file = path.join(root, "userdata", "22", "config", "shortcuts.vdf");
  const first = path.join(scratch("bin"), "battlefront-eternal");
  touch(first, "#!/bin/sh\n", 0o755);
  let record = null;
  const make = (launcher) => createSteamIntegration({ settings: () => record, save: async (next) => { record = next; }, artwork: null, platform: "linux", home: scratch("home"), env: { BFE_STEAM_LAUNCHER: launcher },
    installs: async () => [{ root, kind: "native" }], running: async () => ({ running: false }), run: () => {} });
  await make(first).add();
  assert.deepEqual({ exe: record.exe, launchOptions: record.launchOptions }, { exe: `"${first}"`, launchOptions: STEAM_ARG });
  const edit = (change) => {
    const list = parseShortcuts(readFileSync(file));
    for (const field of list.entries[0].fields) if (change[field.key] !== undefined) field.value = change[field.key];
    list.entries[0].raw = null;
    writeFileSync(file, serializeShortcuts(list));
  };
  // Their own options around ours: kept, also when the launcher moves.
  edit({ LaunchOptions: `gamemoderun %command% ${STEAM_ARG}` });
  assert.equal(await make(first).maintain(), "current");
  const moved = path.join(scratch("bin"), "battlefront-eternal");
  touch(moved, "#!/bin/sh\n", 0o755);
  assert.equal(await make(moved).maintain(), "updated");
  assert.deepEqual([describeShortcut(parseShortcuts(readFileSync(file)).entries[0]).exe, describeShortcut(parseShortcuts(readFileSync(file)).entries[0]).launchOptions], [`"${moved}"`, `gamemoderun %command% ${STEAM_ARG}`]);
  // Their own program in the entry: left alone.
  edit({ Exe: '"/usr/bin/my-wrapper"' });
  const before = readFileSync(file);
  assert.equal(await make(first).maintain(), "theirs");
  assert.ok(readFileSync(file).equals(before));
});

test("Steam is not closed under a game: this one when Steam started it, or another", { skip: !linux && "paths" }, async () => {
  const root = steamRoot(scratch("steam"), [[22, "j", true, 1]]);
  const launcher = path.join(scratch("bin"), "battlefront-eternal");
  touch(launcher, "#!/bin/sh\n", 0o755);
  const make = (now, bySteam = () => false) => createSteamIntegration({ settings: () => null, save: async () => {}, artwork: null, platform: "linux", home: scratch("home"), env: { BFE_STEAM_LAUNCHER: launcher }, bySteam,
    installs: async () => [{ root, kind: "native" }], running: async () => now, run: () => { throw new Error("Steam was asked to close"); } });
  await assert.rejects(make({ running: true, kind: "native", account: "22", game: 0 }, () => true).add({ restart: true }), /Steam started this game/);
  await assert.rejects(make({ running: true, kind: "native", account: "22", game: 440 }).add({ restart: true }), /A game is running in Steam/);
  await assert.rejects(createSteamIntegration({ settings: () => null, save: async () => {}, artwork: null, platform: "linux", home: scratch("home"), env: { BFE_STEAM_LAUNCHER: launcher, XDG_CURRENT_DESKTOP: "gamescope" },
    installs: async () => [{ root, kind: "native" }], running: async () => ({ running: true, kind: "native", account: "22", game: 0 }), run: () => {} }).add({ restart: true }), /session/);
});

test("the boot reads the steam record from the desktop settings", () => {
  const dir = scratch("userdata");
  assert.equal(readSteamSettings(dir), null);
  writeFileSync(path.join(dir, "desktop-settings.json"), JSON.stringify({ steam: { appid: 5, root: "/r" } }));
  assert.deepEqual(readSteamSettings(dir), { appid: 5, root: "/r" });
});

test("a server hosted from a game Steam started runs under the user's systemd, or as usual without one", async () => {
  const { startOutside } = await import("../electron/host.js");
  const { EventEmitter } = await import("node:events");
  const calls = [];
  const fake = (code, error) => (file, args) => {
    calls.push([file, ...args]);
    const child = new EventEmitter();
    setTimeout(() => (error ? child.emit("error", error) : child.emit("exit", code)), 5);
    return child;
  };
  const unit = await startOutside({ executable: "/opt/Battlefront Eternal/battlefront-eternal", args: ["/srv/main.js", "--serve"], env: { ELECTRON_RUN_AS_NODE: "1", PORT: "18460" }, log: "/home/j/.config/Battlefront Eternal/hosted-server/server.log", run: fake(0) });
  assert.match(unit, /^battlefront-eternal-server-\d+$/);
  assert.deepEqual(calls[0], ["systemd-run", "--user", "--collect", "--quiet", `--unit=${unit}`, "--property=StandardOutput=append:/home/j/.config/Battlefront Eternal/hosted-server/server.log",
    "--property=StandardError=append:/home/j/.config/Battlefront Eternal/hosted-server/server.log", "--setenv=ELECTRON_RUN_AS_NODE=1", "--setenv=PORT=18460", "/opt/Battlefront Eternal/battlefront-eternal", "/srv/main.js", "--serve"]);
  assert.equal(await startOutside({ executable: "x", args: [], env: {}, log: "l", run: fake(1) }), null, "no user systemd");
  assert.equal(await startOutside({ executable: "x", args: [], env: {}, log: "l", run: fake(null, new Error("ENOENT")) }), null, "no systemd-run");
});

test("started by Steam on Linux, the game starts again without Steam's overlay, its layer and its runtime's libraries", () => {
  const env = { HOME: "/home/j", SteamGameId: "1", LD_PRELOAD: ":/s/ubuntu12_32/gameoverlayrenderer.so:/s/ubuntu12_64/gameoverlayrenderer.so", ENABLE_VK_LAYER_VALVE_steam_overlay_1: "1",
    LD_LIBRARY_PATH: "/s/ubuntu12_32/steam-runtime/pinned_libs_64:/opt/cuda/lib64:/s/ubuntu12_64" };
  assert.deepEqual(withoutSteamOverlay(env, "linux"), { HOME: "/home/j", SteamGameId: "1", LD_LIBRARY_PATH: "/opt/cuda/lib64", BFE_STEAM_CLEAN: "1" });
  // The player's own preloads stay.
  assert.equal(withoutSteamOverlay({ LD_PRELOAD: "/usr/lib/libgamemodeauto.so.0 /s/ubuntu12_64/gameoverlayrenderer.so" }, "linux").LD_PRELOAD, "/usr/lib/libgamemodeauto.so.0");
  // Nothing of Steam's, once already, or not Linux: no second start.
  assert.equal(withoutSteamOverlay({ LD_PRELOAD: "/usr/lib/libgamemodeauto.so.0" }, "linux"), null);
  assert.equal(withoutSteamOverlay({ ...env, BFE_STEAM_CLEAN: "1" }, "linux"), null);
  assert.equal(withoutSteamOverlay(env, "win32"), null);
  assert.equal(canRestartSelf(process.execPath), process.platform === "linux");
  assert.equal(canRestartSelf("/nonexistent/battlefront-eternal"), false);
});

test("the process Steam started stays where Steam cannot follow a hand-off, and an old launcher that did not is noticed", () => {
  const steam = ["exe", STEAM_ARG];
  assert.equal(steamStays({ argv: steam, env: {}, platform: "win32" }), true, "Windows");
  assert.equal(steamStays({ argv: steam, env: {}, platform: "linux" }), true, "a Flatpak Steam's host spawn passes no Steam environment");
  assert.equal(steamStays({ argv: steam, env: { SteamGameId: "1" }, platform: "linux" }), false, "a native Steam's reaper follows");
  assert.equal(steamStays({ argv: ["exe"], env: {}, platform: "win32" }), false, "not started by Steam");
  assert.equal(steamLost({ argv: steam, env: { BFE_HANDOFF: "1" }, platform: "win32" }), true, "handed off by a launcher that left");
  assert.equal(steamLost({ argv: steam, env: { BFE_HANDOFF: "1", BFE_STEAM_PARENT: "42" }, platform: "win32" }), false);
  assert.equal(steamLost({ argv: steam, env: {}, platform: "win32" }), false, "the process Steam started itself");
  assert.equal(steamLost({ argv: steam, env: { BFE_HANDOFF: "1", SteamGameId: "1" }, platform: "linux" }), false);
});

test("the Steam overlay: on Windows when Steam started the game and the player turned it on; never on Linux", async () => {
  const { OVERLAY_SWITCHES, overlayLoaded, overlaySupport, overlaySwitches } = await import("../electron/steam/overlay.js");
  const bySteam = { argv: ["exe", STEAM_ARG], env: {} };
  assert.deepEqual(overlaySwitches({ steam: { overlay: true }, platform: "win32", ...bySteam }), OVERLAY_SWITCHES);
  assert.deepEqual(OVERLAY_SWITCHES, [["in-process-gpu"], ["disable-direct-composition"]]);
  assert.deepEqual(overlaySwitches({ steam: { overlay: false }, platform: "win32", ...bySteam }), [], "the player's choice");
  assert.deepEqual(overlaySwitches({ steam: {}, platform: "win32", ...bySteam }), [], "off until turned on");
  assert.deepEqual(overlaySwitches({ steam: { overlay: true }, platform: "win32", argv: ["exe"], env: {} }), [], "started outside Steam");
  assert.deepEqual(overlaySwitches({ steam: { overlay: true }, platform: "linux", ...bySteam }), [], "Linux");
  assert.deepEqual(overlaySupport({ platform: "linux", env: { WAYLAND_DISPLAY: "wayland-1" } }), { supported: false, reason: "wayland" });
  assert.deepEqual(overlaySupport({ platform: "linux", env: { DISPLAY: ":0" } }), { supported: false, reason: "x11" });
  assert.deepEqual(overlaySupport({ platform: "win32" }), { supported: true });
  assert.equal(overlayLoaded(() => ({ sharedObjects: ["C:\\Windows\\System32\\ntdll.dll", "C:\\Program Files (x86)\\Steam\\GameOverlayRenderer64.dll"] })), true);
  assert.equal(overlayLoaded(() => ({ sharedObjects: ["/home/j/.local/share/Steam/ubuntu12_64/gameoverlayrenderer.so"] })), true);
  assert.equal(overlayLoaded(() => ({ sharedObjects: ["/usr/lib/libc.so.6"] })), false);
  assert.equal(overlayLoaded(() => { throw new Error("no report"); }), false);
});

test("the player's overlay choice survives adding and removing the library entry", { skip: !linux && "paths" }, async () => {
  const root = steamRoot(scratch("steam"), [[22, "j", true, 1]]);
  const launcher = path.join(scratch("bin"), "battlefront-eternal");
  touch(launcher, "#!/bin/sh\n", 0o755);
  let record = { overlay: true };
  const steam = createSteamIntegration({ settings: () => record, save: async (next) => { record = next; }, artwork: null, platform: "linux", home: scratch("home"), env: { BFE_STEAM_LAUNCHER: launcher },
    installs: async () => [{ root, kind: "native" }], running: async () => ({ running: false }), run: () => {} });
  await steam.add();
  assert.equal(record.overlay, true);
  await steam.remove();
  assert.equal(record.overlay, true);
});
