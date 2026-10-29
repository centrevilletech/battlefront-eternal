// The desktop client's activity for friends, end to end in Electron, with
// stand-ins for what this machine may not have: a Discord client's IPC
// socket (the protocol as Discord speaks it) and a Steam installation's
// folders (not running). Needs prepared game data:
//
//   BFE_CONTENT_DIR=/path/to/game-data PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs \
//     xvfb-run -a node tests/desktop-presence-electron.mjs
//
// Checks: the first menu offers the Steam library once and adding writes
// the entry (artwork included); Discord hears the menu, the Instant Action
// battle with the catalog's map name, the battle's start and side, the
// player's choice of less detail; quitting clears it.
import assert from "node:assert/strict";
import { promises as fs } from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { encodeFrame, FrameReader, OP } from "../electron/presence/discord.js";
import { describeShortcut, STEAM_ARG } from "../electron/steam/shortcut.js";
import { parseShortcuts } from "../electron/steam/vdf.js";

const { _electron } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");
const root = fileURLToPath(new URL("../", import.meta.url));
const content = process.env.BFE_CONTENT_DIR;
if (!content) throw new Error("Set BFE_CONTENT_DIR to prepared game data");
const temp = await fs.mkdtemp(path.join(os.tmpdir(), "bfe-presence-e2e-"));
const CLIENT_ID = "123456789012345678";

// The Discord client's end of the socket.
const runtime = path.join(temp, "run");
await fs.mkdir(runtime, { recursive: true, mode: 0o700 });
const frames = [];
const sockets = new Set();
const discord = net.createServer((socket) => {
  sockets.add(socket);
  const reader = new FrameReader();
  socket.on("data", (chunk) => {
    for (const frame of reader.push(chunk)) {
      frames.push(frame);
      if (frame.op === OP.HANDSHAKE) socket.write(encodeFrame(OP.FRAME, { cmd: "DISPATCH", evt: "READY", data: { v: 1, user: { id: "1", username: "john_doe", global_name: "John Doe" } } }));
      else if (frame.payload?.cmd === "SET_ACTIVITY") socket.write(encodeFrame(OP.FRAME, { cmd: "SET_ACTIVITY", nonce: frame.payload.nonce, data: frame.payload.args.activity ?? null }));
    }
  });
  socket.on("close", () => sockets.delete(socket));
});
await new Promise((resolve) => discord.listen(path.join(runtime, "discord-ipc-0"), resolve));
const activities = () => frames.filter((frame) => frame.payload?.cmd === "SET_ACTIVITY").map((frame) => frame.payload.args.activity ?? null);
const until = async (what, check, ms = 60000) => {
  const end = Date.now() + ms;
  while (!check()) {
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}; activities: ${JSON.stringify(activities())}`);
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
};

// A Steam installation with one signed-in account, closed.
const home = path.join(temp, "home"), data = path.join(temp, "user-data");
const steamRoot = path.join(home, ".local", "share", "Steam");
await fs.mkdir(path.join(steamRoot, "userdata", "22", "config"), { recursive: true });
await fs.mkdir(path.join(steamRoot, "steamapps"), { recursive: true });
await fs.mkdir(path.join(steamRoot, "config"), { recursive: true });
await fs.writeFile(path.join(steamRoot, "config", "loginusers.vdf"), `"users"\n{\n\t"${76561197960265728n + 22n}"\n\t{\n\t\t"AccountName"\t\t"john_doe"\n\t\t"PersonaName"\t\t"John Doe"\n\t\t"MostRecent"\t\t"1"\n\t\t"Timestamp"\t\t"1760000000"\n\t}\n}\n`);
const launcher = path.join(home, ".local", "bin", "battlefront-eternal");
await fs.mkdir(path.dirname(launcher), { recursive: true });
await fs.writeFile(launcher, "#!/bin/sh\n# Created by the Battlefront Eternal installer.\n", { mode: 0o755 });
await fs.mkdir(data, { recursive: true });

let app;
try {
  const bootstrap = path.join(temp, "bootstrap.cjs");
  await fs.writeFile(bootstrap, `const {app}=require('electron'); app.setPath('home',${JSON.stringify(home)}); app.setPath('userData',${JSON.stringify(data)}); import(${JSON.stringify(pathToFileURL(path.join(root, "electron/main.js")).href)});`);
  app = await _electron.launch({
    executablePath: process.env.BFE_ELECTRON || path.join(root, "node_modules/electron/dist/electron"),
    args: [bootstrap, "--no-sandbox", "--use-angle=swiftshader", "--enable-unsafe-swiftshader"],
    env: { ...process.env, HOME: home, XDG_RUNTIME_DIR: runtime, BFE_CONTENT_DIR: content, BFE_DISCORD_APP_ID: CLIENT_ID, BFE_STEAM_LAUNCHER: launcher, BFE_NO_UPDATE: "1" },
    timeout: 60000,
  });
  const page = await app.firstWindow();
  await page.waitForURL((url) => url.protocol === "app:" && !url.pathname.includes("__desktop"), { timeout: 60000 });

  // Discord: the handshake with the application id, then the front end.
  await until("the handshake", () => frames.some((frame) => frame.op === OP.HANDSHAKE));
  assert.deepEqual(frames.find((frame) => frame.op === OP.HANDSHAKE).payload, { v: 1, client_id: CLIENT_ID });
  await until("the menus", () => activities().some((activity) => activity?.details === "In the menus"));

  // The click gate (pressed until it answers), then the main menu offers
  // the Steam library once.
  const dialog = page.getByRole("dialog", { name: "Show Steam friends when you play" });
  for (let tries = 0; !(await dialog.isVisible()); tries++) {
    if (tries > 30) throw new Error("the Steam offer never showed");
    if (await page.evaluate(() => !!window.__shell?.gate)) await page.mouse.click(400, 300);
    await page.waitForTimeout(1000);
  }
  assert.match(await dialog.textContent(), /Steam account: John Doe\./);
  await dialog.getByRole("button", { name: "Add to Steam" }).click();
  await dialog.waitFor({ state: "detached" });
  const file = path.join(steamRoot, "userdata", "22", "config", "shortcuts.vdf");
  const entry = describeShortcut(parseShortcuts(await fs.readFile(file)).entries[0]);
  assert.equal(entry.name, "Battlefront Eternal");
  assert.equal(entry.exe, `"${launcher}"`);
  assert.equal(entry.launchOptions, STEAM_ARG);
  assert.equal(entry.overlay, false);
  for (const suffix of [".jpg", "p.jpg", "_hero.jpg", "_logo.png", "_icon.png", ".json"]) await fs.access(path.join(steamRoot, "userdata", "22", "config", "grid", `${entry.appid}${suffix}`));
  const settings = JSON.parse(await fs.readFile(path.join(data, "desktop-settings.json"), "utf8"));
  assert.deepEqual(settings.steam, { appid: entry.appid, root: await fs.realpath(steamRoot), kind: "native", account: "22", launch: true, overlay: false,
    exe: entry.exe, startDir: entry.startDir, launchOptions: STEAM_ARG, grantedAccess: false });
  const state = await page.evaluate(() => window.bfeDesktop.integrations.request("state"));
  assert.equal(state.steam.added, true);
  // Linux: no Steam overlay, and the page is told why.
  assert.deepEqual(state.steam.overlay, { supported: false, reason: "x11", on: false, switches: false, active: false });
  assert.deepEqual({ state: state.discord.state, user: state.discord.user, detail: state.discord.detail }, { state: "connected", user: { name: "John Doe" }, detail: "full" });

  // An Instant Action battle: the catalog's map name at once, then the
  // battle's start and the player's side.
  await page.evaluate(() => location.assign("/?map=nab2&era=cw&rotation=nab2:cw&round=0&side=rep&ai=off"));
  await until("the battle", () => activities().some((activity) => activity?.details === "Instant Action: Naboo: Theed"));
  const shown = activities().find((activity) => activity?.details === "Instant Action: Naboo: Theed");
  assert.equal(shown.state, "Clone Wars, Republic");
  const loadingStart = shown.timestamps.start;
  await until("the battle's start", () => activities().some((activity) => activity?.details === "Instant Action: Naboo: Theed" && activity.timestamps.start > loadingStart), 240000);

  // Less detail, then none: Discord is let go.
  await page.evaluate(() => window.bfeDesktop.integrations.request("discord", "game"));
  await until("game only", () => { const last = activities().at(-1); return last && !last.details && last.timestamps; });
  await page.evaluate(() => window.bfeDesktop.integrations.request("discord", "off"));
  await until("cleared", () => activities().at(-1) === null && sockets.size === 0);
  console.log("PASS:", activities().length, "activities; Steam entry", entry.appid);
} finally {
  await app?.close().catch(() => {});
  for (const socket of sockets) socket.destroy();
  discord.close();
  await fs.rm(temp, { recursive: true, force: true });
}
