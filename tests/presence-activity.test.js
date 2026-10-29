import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { after, test } from "node:test";
import { discordActivity, fit, sceneOf, titleCase } from "../electron/presence/activity.js";
import { cleanReport, cleanServer, createPresence, serverName } from "../electron/presence/index.js";
import { followBattle } from "../src/presence.js";

const temp = mkdtempSync(path.join(tmpdir(), "bfe-presence-"));
after(() => rmSync(temp, { recursive: true, force: true }));
const SERVER = "a".repeat(64);
const ORIGIN = "app://bfe";

test("the page's URL says whether it is the loader, the front end or a battle, and which", () => {
  assert.deepEqual(sceneOf(`${ORIGIN}/__desktop/prepare.html`), { scene: "loader" });
  assert.deepEqual(sceneOf(`${ORIGIN}/`), { scene: "menu" });
  assert.deepEqual(sceneOf(`${ORIGIN}/?screen=main`), { scene: "menu" });
  assert.deepEqual(sceneOf(`${ORIGIN}/?shell=1&map=nab2`), { scene: "menu" });
  assert.deepEqual(sceneOf(`${ORIGIN}/?map=nab2&era=cw&rotation=nab2:cw,geo1:cw&round=0`), { scene: "battle", map: "nab2", era: "cw", side: null, network: null });
  assert.deepEqual(sceneOf(`${ORIGIN}/?map=tat2&side=imp&era=gcw&network=1&server=${SERVER}`), { scene: "battle", map: "tat2", era: "gcw", side: "imp", network: { server: SERVER, lan: false } });
  assert.deepEqual(sceneOf(`${ORIGIN}/?map=nab2&network=1&server=${SERVER}&address=192.168.1.4&port=18451`).network, { server: SERVER, lan: true });
  // Nothing odd from the URL reaches the text.
  assert.equal(sceneOf(`${ORIGIN}/?map=${"x".repeat(40)}&network=1&server=nope`).map, null);
  assert.equal(sceneOf(`${ORIGIN}/?map=nab2&network=1&server=nope`).network.server, null);
  assert.deepEqual(sceneOf("not a url"), { scene: "unknown" });
});

test("text fits Discord's 2 to 128 characters, and the catalog's capitals read as names", () => {
  assert.equal(fit("x"), undefined);
  assert.equal(fit("  a  b  "), "a b");
  assert.equal(fit("y".repeat(200)).length, 128);
  assert.ok(fit("y".repeat(200)).endsWith("…"));
  assert.equal(titleCase("NABOO: THEED"), "Naboo: Theed");
  assert.equal(titleCase("YAVIN 4: TEMPLE"), "Yavin 4: Temple");
  assert.equal(titleCase("Bespin: Cloud City"), "Bespin: Cloud City");
});

test("what friends see in each scene, at each level of detail", () => {
  const started = 1_760_000_000_000, since = started + 60_000;
  const base = { started, since };
  assert.deepEqual(discordActivity({ ...base, scene: "loader", loader: "preparing" }), { timestamps: { start: started }, details: "Preparing game data" });
  assert.deepEqual(discordActivity({ ...base, scene: "loader", loader: "update" }).details, "Updating the game");
  assert.deepEqual(discordActivity({ ...base, scene: "menu", screen: "main" }), { timestamps: { start: started }, details: "In the menus" });
  assert.equal(discordActivity({ ...base, scene: "menu", screen: "mp" }).state, "Looking for a server");
  assert.deepEqual(discordActivity({ ...base, scene: "battle", map: "nab2", mapName: "NABOO: THEED", era: "cw", side: "rep", network: null }),
    { timestamps: { start: since }, details: "Instant Action: Naboo: Theed", state: "Clone Wars, Republic" });
  // No catalog: the map's id; no side picked yet: the era alone.
  assert.deepEqual(discordActivity({ ...base, scene: "battle", map: "geo1", era: "cw", network: null }), { timestamps: { start: since }, details: "Instant Action: GEO1", state: "Clone Wars" });
  const online = { ...base, scene: "battle", map: "tat2", mapName: "Tatooine: Mos Eisley", era: "gcw", side: "imp", network: { server: SERVER, lan: false }, server: { id: SERVER, name: "Planet Lightwave", players: 3, maxPlayers: 64 }, partyId: "p1" };
  assert.deepEqual(discordActivity(online), { timestamps: { start: since }, details: "Online: Tatooine: Mos Eisley", state: "Planet Lightwave", party: { id: "p1", size: [3, 64] } });
  // The battle's own count wins over the list's.
  assert.deepEqual(discordActivity({ ...online, players: 12 }).party.size, [12, 64]);
  // An impossible count shows no party.
  assert.equal(discordActivity({ ...online, players: 99 }).party, undefined);
  assert.equal(discordActivity({ ...online, network: { server: SERVER, lan: true } }).details, "LAN: Tatooine: Mos Eisley");
  assert.equal(discordActivity({ ...online, server: { ...online.server, hosted: true } }).details, "Hosting: Tatooine: Mos Eisley");
  // A server not in any list: the era and side instead of its name.
  assert.deepEqual(discordActivity({ ...online, server: null }), { timestamps: { start: since }, details: "Online: Tatooine: Mos Eisley", state: "Galactic Civil War, Empire" });
  // Less detail: only that the game runs; none at all.
  assert.deepEqual(discordActivity(online, { detail: "game" }), { timestamps: { start: started } });
  // The application's picture, at either level.
  assert.deepEqual(discordActivity(online, { detail: "game", image: "logo" }), { timestamps: { start: started }, assets: { large_image: "logo", large_text: "Battlefront Eternal" } });
  assert.deepEqual(discordActivity(online, { image: "logo" }).assets, { large_image: "logo", large_text: "Battlefront Eternal" });
  assert.equal(discordActivity(online, { detail: "off" }), null);
});

test("pages' reports and servers are checked before anything of them is shown", () => {
  assert.deepEqual(cleanReport({ screen: "mp", side: "rep", players: 5, phase: "playing", extra: "x" }), { screen: "mp", side: "rep", players: 5, phase: "playing" });
  assert.deepEqual(cleanReport({ screen: "<b>", side: "republic", players: -1, phase: "won" }), {});
  assert.deepEqual(cleanReport(null), {});
  assert.deepEqual(cleanServer({ id: SERVER, name: "n".repeat(100), players: [1, 2], maxPlayers: 64, address: "10.0.0.1" }), { id: SERVER, name: "n".repeat(64), players: 2, maxPlayers: 64, hosted: false });
  assert.equal(cleanServer({ id: "short", name: "x" }), null);
  // Another person's text on the player's profile: printable, one line.
  assert.equal(serverName("Planet\u202E evahnoc\u200F\n Lightwave\u0007"), "Planet evahnoc Lightwave");
  assert.equal(serverName("\u200B\u2066"), null);
  assert.equal(serverName("ü".repeat(80)).length, 64);
  assert.equal(serverName("🙂".repeat(80)), "🙂".repeat(64), "never half a character");
});

test("the presence follows the window and the pages, and publishes only changes", async () => {
  const content = path.join(temp, "content");
  mkdirSync(path.join(content, "common", "shell"), { recursive: true });
  writeFileSync(path.join(content, "common", "shell", "shell.json"), JSON.stringify({ maps: [{ id: "nab2", name: "Naboo: Theed" }, { id: "tat2", name: "Tatooine: Mos Eisley" }] }));
  const shown = [];
  let clock = 1_000_000;
  const settings = { discord: "full" };
  const presence = createPresence({ settings: () => settings, contentRoot: () => content, discord: { setActivity: (activity) => shown.push(activity), stop() {} }, now: () => clock });
  presence.loader("preparing");
  assert.equal(shown.at(-1).details, "Preparing game data");
  await presence.navigated(`${ORIGIN}/?screen=main`);
  assert.equal(shown.at(-1).details, "In the menus");
  presence.report({ screen: "mp" });
  assert.equal(shown.at(-1).state, "Looking for a server");
  presence.report({ screen: "mp" });
  presence.report({ side: "rep" });
  assert.equal(shown.length, 3);
  presence.servers([{ id: SERVER, name: "Planet Lightwave", players: 5, maxPlayers: 64 }]);
  clock += 5000;
  await presence.navigated(`${ORIGIN}/?map=tat2&side=imp&era=gcw&network=1&server=${SERVER}`);
  const joined = shown.at(-1);
  assert.equal(joined.details, "Online: Tatooine: Mos Eisley");
  assert.equal(joined.state, "Planet Lightwave");
  assert.deepEqual(joined.party.size, [5, 64]);
  assert.match(joined.party.id, /^[0-9a-f]{32}$/);
  assert.notEqual(joined.party.id, SERVER.slice(0, 32));
  // The clock starts when the battle does, not while it loads.
  clock += 9000;
  presence.report({ phase: "playing", players: 7 });
  assert.equal(shown.at(-1).timestamps.start, clock);
  assert.deepEqual(shown.at(-1).party.size, [7, 64]);
  // The player turns details off, then the activity off.
  settings.discord = "game";
  presence.refresh();
  assert.deepEqual(shown.at(-1), { timestamps: { start: 1_000_000 } });
  settings.discord = "off";
  presence.refresh();
  assert.equal(shown.at(-1), null);
});

test("a battle reports its start, the player's side as it changes and, online, the players in", () => {
  const reports = [];
  const handlers = {};
  let team = 1, players = [{}, {}];
  const timers = [];
  const spawnPort = {
    humanState: () => ({ team }),
    teams: () => ({ 1: { side: "rep" }, 2: { side: "cis" } }),
    on: (event, fn) => { handlers[event] = fn; return () => delete handlers[event]; },
  };
  const stop = followBattle({ spawnPort, scoreboardPort: { players: () => players }, report: (r) => reports.push(r), setTimer: (fn) => (timers.push(fn), timers.length), clearTimer: () => timers.splice(0) });
  assert.deepEqual(reports, [{ phase: "playing" }, { side: "rep" }, { players: 2 }]);
  team = 2;
  handlers.teamChanged();
  assert.deepEqual(reports.at(-1), { side: "cis" });
  players = [{}, {}, {}];
  timers[0]();
  assert.deepEqual(reports.at(-1), { players: 3 });
  timers[0]();
  assert.equal(reports.length, 5);
  stop();
  assert.deepEqual(Object.keys(handlers), []);
  // Offline the scoreboard lists no players: nothing is reported for them.
  const offline = [];
  followBattle({ spawnPort, scoreboardPort: { players: () => null }, report: (r) => offline.push(r), setTimer: () => 0, clearTimer() {} });
  assert.ok(!offline.some((r) => "players" in r));
});

test("Discord's overlay keys open Discord, not the stats line", async () => {
  const { overlayChord } = await import("../src/input.js");
  assert.equal(overlayChord({ code: "Backquote", shiftKey: true, ctrlKey: false }), true);
  assert.equal(overlayChord({ code: "Backquote", shiftKey: false, ctrlKey: true }), true);
  assert.equal(overlayChord({ code: "Backquote", shiftKey: false, ctrlKey: false }), false);
  assert.equal(overlayChord({ code: "Tab", shiftKey: true, ctrlKey: false }), false);
});

test("STEAM & DISCORD says what is set up, and the offer asks only when it can help", async () => {
  const { discordText, steamText, integrationsPage } = await import("../src/shell/integrations.js");
  assert.equal(discordText({ state: "unavailable" }), "NOT IN THIS BUILD");
  assert.equal(discordText({ state: "connected", detail: "full", user: { name: "John Doe" } }), "CONNECTED: JOHN DOE");
  assert.equal(discordText({ state: "searching", detail: "full" }), "DISCORD NOT RUNNING");
  assert.equal(discordText({ state: "connected", detail: "off" }), "OFF");
  assert.equal(steamText({ available: false, problem: "no-steam" }), "STEAM NOT FOUND");
  assert.equal(steamText({ available: true, added: false, problem: "snap" }), "SNAP STEAM CANNOT START IT");
  assert.equal(steamText({ available: true, added: false, problem: null }), "ADD TO STEAM");
  assert.equal(steamText({ available: true, added: true }), "IN LIBRARY: REMOVE");
  // Nothing to offer: no dialog (there is no document here to show one).
  // A fresh module each time: the offer is made once per page.
  for (const steam of [{ available: false }, { available: true, added: true }, { available: true, declined: true }, { available: true, problem: "snap" }, { available: true, startedBySteam: true }]) {
    const asked = [];
    const bridge = { request: async (action) => { asked.push(action); return { steam }; } };
    assert.equal(await (await import(`../src/shell/integrations.js?${Math.random()}`)).offerSteam(bridge), false);
    assert.deepEqual(asked, ["state"]);
  }
  // The page: its rows follow the state the app reports.
  const calls = [];
  const bridge = { request: async (action, value) => { calls.push([action, value]); return action === "state" ? { discord: { detail: "game", state: "connected", user: { name: "J" } }, steam: { available: true, added: true, launch: true } } : true; } };
  const page = integrationsPage({ bridge, every: 60000 });
  page.enter({ forward: true });
  await new Promise((resolve) => setTimeout(resolve, 10));
  const rows = Object.fromEntries(page.widget("rows").rows.map((row) => [row.id, row]));
  assert.equal(rows.discord.text(), "GAME ONLY");
  assert.equal(rows.steam.text(), "IN LIBRARY: REMOVE");
  assert.equal(rows["steam-launch"].hidden(), false);
  rows.discord.stepBy(1);
  await new Promise((resolve) => setTimeout(resolve, 10));
  assert.deepEqual(calls.find(([action]) => action === "discord"), ["discord", "off"]);
  page.exit();
});

test("STEAM & DISCORD shows the overlay's toggle on Windows, its state when Steam started it, and why not on Linux", async () => {
  const { integrationsPage } = await import("../src/shell/integrations.js");
  const page = async (steam) => {
    const bridge = { request: async (action) => (action === "state" ? { discord: { detail: "full", state: "searching" }, steam } : true) };
    const screen = integrationsPage({ bridge, every: 60000 });
    screen.enter({ forward: true });
    await new Promise((resolve) => setTimeout(resolve, 10));
    const rows = Object.fromEntries(screen.widget("rows").rows.map((row) => [row.id, row]));
    screen.exit();
    return rows;
  };
  const windows = await page({ available: true, added: true, overlay: { supported: true, on: true, switches: true, active: false } });
  assert.equal(windows["steam-overlay"].hidden(), false);
  assert.equal(windows["steam-overlay"].text(), "ON");
  assert.equal(windows["steam-overlay-now"].hidden(), false);
  assert.equal(windows["steam-overlay-now"].text(), "NOT LOADED");
  assert.equal(windows["steam-overlay-linux"].hidden(), true);
  const linux = await page({ available: true, added: true, overlay: { supported: false, reason: "wayland", on: false, switches: false, active: false } });
  assert.equal(linux["steam-overlay"].hidden(), true);
  assert.equal(linux["steam-overlay-now"].hidden(), true);
  assert.equal(linux["steam-overlay-linux"].hidden(), false);
  assert.equal(linux["steam-overlay-linux"].text(), "NOT ON LINUX");
});
