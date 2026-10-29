import test from "node:test";
import assert from "node:assert/strict";
import { ScreenStack } from "../src/menus/index.js";
import { ServerList, serverBrowser, compatible, errorText } from "../src/shell/server-browser.js";
import { NETWORK_CONTRACT } from "../src/runtime/network-contract.js";

const server = (id, name, extra = {}) => ({
  id, name, players: 0, maxPlayers: 64, latencyMs: 50, location: "Alabama, US",
  simulation: NETWORK_CONTRACT.simulation, protocol: NETWORK_CONTRACT.protocol,
  battle: { map: "nab2", mapName: "NABOO: THEED", era: "cw", mode: "conquest" }, ...extra,
});
const LIST = [
  server("a", "Alpha", { players: 12, latencyMs: 90 }),
  server("b", "Bravo", { players: 64, latencyMs: 20, battle: { map: "hot1", mapName: "HOTH: ECHO BASE", era: "gcw", mode: "conquest" } }),
  server("c", "Charlie", { players: 0, latencyMs: null, location: "Frankfurt, DE" }),
  server("d", "Delta", { players: 3, latencyMs: 140, simulation: "bfe-sim-1" }),
];
const loaded = async (servers = LIST) => {
  const list = new ServerList({ load: async () => ({ servers }) });
  await list.refresh();
  return list;
};

test("the list sorts on ping by default, unknown pings last, and keeps the first picked", async () => {
  const list = await loaded();
  assert.equal(list.state, "ready");
  assert.deepEqual(list.rows().map((s) => s.id), ["b", "a", "d", "c"]);
  assert.equal(list.selected().id, "b");
});

test("a header sorts on its column, again the other way; players start with the fullest", async () => {
  const list = await loaded();
  list.sortBy("players");
  assert.deepEqual(list.rows().map((s) => s.id), ["b", "a", "d", "c"]);
  list.sortBy("players");
  assert.deepEqual(list.rows().map((s) => s.id), ["c", "d", "a", "b"]);
  list.sortBy("name");
  assert.deepEqual(list.rows().map((s) => s.id), ["a", "b", "c", "d"]);
});

test("filters: name, map, era, ping, full and empty; the pick moves off a filtered server", async () => {
  const list = await loaded();
  list.setFilter("name", "hoth");
  assert.deepEqual(list.rows().map((s) => s.id), ["b"], "a map name matches too");
  list.setFilter("name", "frankfurt");
  assert.deepEqual(list.rows().map((s) => s.id), ["c"], "and a place");
  list.setFilter("name", "");
  list.setFilter("era", "gcw");
  assert.deepEqual(list.rows().map((s) => s.id), ["b"]);
  list.setFilter("era", "all");
  list.setFilter("ping", "100");
  assert.deepEqual(list.rows().map((s) => s.id), ["b", "a"]);
  list.setFilter("ping", "any");
  list.setFilter("hideFull", true);
  assert.ok(!list.rows().some((s) => s.id === "b"));
  assert.equal(list.selected().id, "a", "the pick moved to the first one shown");
  list.setFilter("hideEmpty", true);
  assert.deepEqual(list.rows().map((s) => s.id), ["a", "d"]);
  list.setFilter("map", "hot1");
  assert.deepEqual(list.rows(), []);
  assert.equal(list.selected(), null);
});

test("keys: Up and Down pick, hand the focus on at the ends; Page Down, End and Home", async () => {
  const list = await loaded();
  const sounds = [];
  const screen = { sound: (kind) => sounds.push(kind) };
  assert.equal(list.key("ArrowUp", 0, screen), false, "Up at the top goes on");
  assert.equal(list.key("ArrowDown", 0, screen), true);
  assert.equal(list.selected().id, "a");
  assert.equal(list.key("End", 0, screen), true);
  assert.equal(list.selected().id, "c");
  assert.equal(list.key("ArrowDown", 0, screen), false, "Down at the bottom goes on");
  list.shown = 3;
  list.key("Home", 0, screen);
  list.key("PageDown", 0, screen);
  assert.equal(list.selected().id, "d");
  assert.deepEqual(sounds, ["select", "select", "select", "select"]);
});

test("joining: the picked server and its source; another version can't be joined", async () => {
  const joined = [];
  const list = new ServerList({ load: async () => ({ servers: LIST }), onJoin: (s, source) => joined.push([s.id, source]) });
  await list.refresh();
  const sounds = [];
  const screen = { sound: (kind) => sounds.push(kind) };
  list.activate(0, screen);
  list.select("d");
  assert.equal(compatible(list.selected()), false);
  list.activate(0, screen);
  assert.deepEqual(joined, [["b", "internet"]]);
  assert.deepEqual(sounds, ["accept", "error"]);
});

test("refreshing: what to show instead of servers, errors, and only the latest answer counts", async () => {
  let answer;
  const list = new ServerList({ load: () => answer() });
  answer = async () => ({ state: "notice", consent: { accepted: false } });
  await list.refresh();
  assert.equal(list.state, "notice");
  answer = async () => { throw new Error("offline"); };
  await list.refresh();
  assert.deepEqual([list.state, list.message], ["error", "OFFLINE"]);
  let release;
  answer = () => new Promise((resolve) => { release = resolve; });
  const first = list.refresh();
  const firstRelease = release;
  answer = async () => ({ servers: [LIST[0]] });
  await list.refresh();
  firstRelease({ servers: LIST });
  await first;
  assert.deepEqual(list.rows().map((s) => s.id), ["a"], "the older answer came too late");
});

test("the source: switching to the LAN asks for its servers and clears the pick", async () => {
  const asked = [];
  const list = new ServerList({ load: async (source) => { asked.push(source); return { servers: source === "lan" ? [LIST[2]] : LIST }; } });
  await list.refresh();
  list.setSource("lan");
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.deepEqual(asked, ["internet", "lan"]);
  assert.equal(list.selected().id, "c");
});

test("the screen: tabs, the list first on the ring, JOIN only with a server picked, CREATE opens hosting", async () => {
  const hosted = [];
  const screen = serverBrowser({ load: async () => ({ servers: [] }), join: () => {}, host: (s) => hosted.push(s.id) });
  const stack = new ScreenStack({ sound() {} });
  stack.push(screen);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(screen.focused.widget.id, "servers");
  assert.equal(screen.widget("join").enabled(0), false);
  screen.widget("sections").choose(1, screen);
  assert.deepEqual(hosted, ["mp"]);
  assert.equal(screen.widget("sections").active, 0, "JOIN stays the tab");
  screen.exit();
});

test("a renewal that fails keeps the list and says so, tries again sooner, and never overlaps another", async () => {
  let now = 0;
  let answer = async () => ({ servers: LIST });
  let calls = 0;
  const list = new ServerList({ load: () => { calls += 1; return answer(); }, now: () => now });
  await list.refresh();
  assert.equal(list.rows().length, 4);
  let release;
  answer = () => new Promise((resolve, reject) => { release = reject; });
  now = 31_000;
  list.tick();
  list.tick();
  list.tick();
  assert.equal(calls, 2, "one renewal at a time");
  release(new Error("Error invoking remote method 'bfe:network': Error: Can't reach the online service (ETIMEDOUT). Check your connection and try again."));
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(list.state, "ready");
  assert.equal(list.rows().length, 4, "the list stays");
  assert.equal(list.stale, "CAN'T REACH THE ONLINE SERVICE (ETIMEDOUT). CHECK YOUR CONNECTION AND TRY AGAIN.");
  answer = async () => ({ servers: [LIST[0]] });
  now = 31_000 + 5_000;
  list.tick();
  assert.equal(calls, 2, "not yet");
  now = 31_000 + 11_000;
  list.tick();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(calls, 3, "again after ten seconds");
  assert.equal(list.stale, "");
  assert.deepEqual(list.rows().map((s) => s.id), ["a"]);
});

test("a first load that fails says why, in words", async () => {
  const list = new ServerList({ load: async () => { throw new Error("Error invoking remote method 'bfe:network': TypeError: fetch failed"); } });
  await list.refresh();
  assert.deepEqual([list.state, list.message], ["error", "FETCH FAILED"]);
  assert.equal(errorText(new Error("Error: Accept the online privacy notice first.")), "Accept the online privacy notice first.");
});
