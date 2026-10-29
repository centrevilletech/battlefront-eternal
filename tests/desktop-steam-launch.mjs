// A packaged build's start from outside Steam goes through the Steam
// library (electron/steam/relaunch.js), against a stand-in Steam: a
// process whose executable is a client's (ubuntu12_32/steam), its
// registry.vdf with the signed-in account, the library entry, and a
// `steam` command that starts the entry as Steam does (its launch options,
// SteamGameId in the environment). Linux only.
//
//   BFE_PACKAGED=release/linux-unpacked BFE_CONTENT_DIR=/path/to/game-data \
//     xvfb-run -a node tests/desktop-steam-launch.mjs
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { promises as fs, readFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { rungameId, STEAM_ARG, upsertShortcut } from "../electron/steam/shortcut.js";

const packaged = path.resolve(process.env.BFE_PACKAGED || "release/linux-unpacked");
const executable = path.join(packaged, "battlefront-eternal");
const content = process.env.BFE_CONTENT_DIR;
if (!content) throw new Error("Set BFE_CONTENT_DIR to prepared game data");
const temp = await fs.mkdtemp(path.join(os.tmpdir(), "bfe-steam-launch-"));
const home = path.join(temp, "home"), config = path.join(home, ".config"), data = path.join(home, ".local", "share");
const steamRoot = path.join(data, "Steam"), bin = path.join(temp, "bin"), calls = path.join(temp, "steam-calls.log");
const children = [];
const alive = (pid) => { try { process.kill(pid, 0); return true; } catch { return false; } };
const cmdline = (pid) => { try { return readFileSync(`/proc/${pid}/cmdline`, "utf8").split("\0"); } catch { return []; } };
const until = async (what, check, ms = 60000) => {
  const end = Date.now() + ms;
  while (!(await check())) {
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
};

try {
  // The library entry, as "Add to Steam" writes it.
  await fs.mkdir(path.join(steamRoot, "userdata", "22", "config"), { recursive: true });
  const added = upsertShortcut(null, { exe: `"${executable}"`, startDir: `"${packaged}"`, launchOptions: STEAM_ARG });
  await fs.writeFile(path.join(steamRoot, "userdata", "22", "config", "shortcuts.vdf"), added.buffer);
  const userData = path.join(config, "Battlefront Eternal");
  await fs.mkdir(userData, { recursive: true });
  await fs.writeFile(path.join(userData, "desktop-settings.json"), JSON.stringify({ steam: { appid: added.appid, root: steamRoot, kind: "native", account: "22", launch: true } }));
  // The signed-in client: its registry and a process with its executable's name.
  await fs.mkdir(path.join(home, ".steam"), { recursive: true });
  await fs.writeFile(path.join(home, ".steam", "registry.vdf"), `"Registry" { "HKCU" { "Software" { "Valve" { "Steam" { "ActiveProcess" { "pid" "1" "ActiveUser" "22" } } } } } }`);
  const client = path.join(steamRoot, "ubuntu12_32", "steam");
  await fs.mkdir(path.dirname(client), { recursive: true });
  await fs.copyFile("/bin/sleep", client);
  await fs.chmod(client, 0o755);
  const steam = spawn(client, ["600"], { stdio: "ignore" });
  children.push(steam.pid);
  // Steam's overlay library, preloaded into what it starts (a harmless
  // library standing in for it).
  const overlay = path.join(steamRoot, "ubuntu12_64", "gameoverlayrenderer.so");
  await fs.mkdir(path.dirname(overlay), { recursive: true });
  await fs.copyFile("/lib/x86_64-linux-gnu/libdl.so.2", overlay);
  // `steam steam://rungameid/<id>`: start the entry as Steam does.
  await fs.mkdir(bin, { recursive: true });
  await fs.writeFile(path.join(bin, "steam"), `#!/bin/sh
echo "$*" >> ${JSON.stringify(calls)}
case "$1" in
  steam://rungameid/${rungameId(added.appid)}) SteamGameId=${rungameId(added.appid)} SteamClientLaunch=1 LD_PRELOAD=":${overlay}" ENABLE_VK_LAYER_VALVE_steam_overlay_1=1 setsid ${JSON.stringify(executable)} ${STEAM_ARG} --no-sandbox >/dev/null 2>&1 & echo "child $!" >> ${JSON.stringify(calls)} ;;
esac
`, { mode: 0o755 });
  const env = { ...process.env, HOME: home, XDG_CONFIG_HOME: config, XDG_DATA_HOME: data, PATH: `${bin}:${process.env.PATH}`, BFE_CONTENT_DIR: content };
  delete env.SteamGameId;
  delete env.SteamAppId;

  // Started from the desktop: it goes through Steam and gets out of the way.
  const begun = Date.now();
  const first = spawn(executable, ["--no-sandbox"], { env, stdio: "ignore" });
  children.push(first.pid);
  const code = await new Promise((resolve) => first.once("exit", resolve));
  assert.equal(code, 0);
  const log = await fs.readFile(calls, "utf8");
  assert.match(log, new RegExp(`^steam://rungameid/${rungameId(added.appid)}$`, "m"));
  const second = Number(/^child (\d+)$/m.exec(log)[1]);
  children.push(second);
  // Chromium rewrites its command line into one string.
  assert.ok(cmdline(second).join(" ").split(" ").includes(STEAM_ARG), "Steam's copy has the entry's argument");
  console.log(`through Steam in ${Date.now() - begun} ms`);
  // Steam's copy carries on to its window, the same process without the
  // overlay preloaded.
  await new Promise((resolve) => setTimeout(resolve, 8000));
  assert.ok(alive(second), "Steam's copy runs");
  // (Chromium rewrites its environment's memory for its process titles, so
  // what is loaded says it: /proc/<pid>/maps.)
  const tree = (pid) => [pid, ...readFileSync(`/proc/${pid}/task/${pid}/children`, "utf8").trim().split(/\s+/).filter(Boolean).flatMap((child) => tree(Number(child)))];
  const processes = tree(second).filter((pid) => { try { return !readFileSync(`/proc/${pid}/stat`, "utf8").split(") ")[1].startsWith("Z"); } catch { return false; } });
  assert.ok(processes.length >= 3, "the game's processes run");
  for (const pid of processes) assert.ok(!readFileSync(`/proc/${pid}/maps`, "utf8").includes("gameoverlayrenderer"), `no overlay in ${cmdline(pid).join(" ").slice(0, 80)}`);
  const updates = await fs.readFile(path.join(data, "battlefront-eternal", "update.log"), "utf8");
  assert.match(updates, /steam: starting through Steam \(native, \d+\)/);
  assert.match(updates, /steam: Steam started the game/);
  process.kill(second, "SIGTERM");
  await until("Steam's copy to close", () => !alive(second), 20000);

  // Steam closed: the game starts by itself, without asking Steam.
  process.kill(steam.pid, "SIGTERM");
  await until("the stand-in Steam to close", () => !alive(steam.pid));
  await fs.writeFile(calls, "");
  const alone = spawn(executable, ["--no-sandbox"], { env, stdio: "ignore" });
  children.push(alone.pid);
  await new Promise((resolve) => setTimeout(resolve, 8000));
  assert.ok(alive(alone.pid), "it runs itself");
  assert.equal(await fs.readFile(calls, "utf8"), "", "Steam was not asked");
  console.log("PASS: through Steam when it runs, directly when it does not");
} finally {
  for (const pid of children) { try { process.kill(pid, "SIGKILL"); } catch {} }
  await fs.rm(temp, { recursive: true, force: true });
}
