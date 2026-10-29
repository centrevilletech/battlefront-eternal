import test from "node:test";
import assert from "node:assert/strict";
import { createScreens } from "../src/shell/screens.js";
import { ScreenStack } from "../src/menus/stack.js";
import { MAX_PLAYLIST } from "../src/launch.js";

// Settings with a profile made (a name of the player's own), as multiplayer
// needs; profile(null) is a new install's.
function profile(name = "KEVIN") {
  const values = new Map(name ? [["playerName", name]] : []);
  return { get: (key) => values.get(key), set: (key, value) => values.set(key, value) };
}
function fixture({ settings = profile(), askProfile } = {}) {
  const visits = [], previews = [];
  const data = { strings: {}, maps: [
    { id: "nab2", name: "THEED", extracted: true, eras: { cw: {}, gcw: {} } },
    { id: "geo1", name: "GEONOSIS", extracted: true, eras: { cw: {} } },
    { id: "hot1", name: "HOTH", extracted: false, eras: { gcw: {} } },
  ] };
  const screens = createScreens({ data, settings, askProfile, media: { preview: (id) => previews.push(id) },
    query: "?screen=main&noreload=1&seed=7&traceInput=1&profileRuntime=1", navigate: (url) => visits.push(url), close: () => visits.push("close") });
  const stack = new ScreenStack({ sound: () => {} });
  stack.push(screens.main());
  return { screens, stack, visits, previews };
}
function choose(stack, id) {
  const screen = stack.top, rows = screen.widget("rows");
  const index = rows.rows.findIndex((row) => row.id === id);
  assert.ok(rows.enabled(index));
  rows.activate(index, screen);
}

test("main navigation, back, disabled campaign and desktop network requirement", () => {
  const { stack } = fixture();
  choose(stack, "sp");
  assert.equal(stack.top.id, "sp");
  assert.equal(stack.top.widget("rows").enabled(0), false);
  stack.key("Escape");
  choose(stack, "mp");
  choose(stack, "lan");
  assert.equal(stack.top.id, "mp-lan");
  assert.equal(stack.top.widget("rows").rows[0].label, "ONLINE PLAY REQUIRES THE DESKTOP CLIENT.");
  stack.key("Escape");
  choose(stack, "online");
  assert.equal(stack.top.id, "mp-online");
});

test("Instant Action adds eras, rejects missing maps, removes and launches with dev flags", () => {
  const { stack, visits, previews } = fixture();
  choose(stack, "sp"); choose(stack, "instant");
  const ia = stack.top, available = ia.widget("available");
  assert.deepEqual(previews, ["nab2"]);
  assert.equal(ia.widget("launch").enabled(), false);
  available.activate(0, ia);
  assert.equal(stack.top.id, "ia-era");
  choose(stack, "gcw");
  available.select(1, ia); available.activate(0, ia);
  assert.deepEqual(ia.entries, [{ map: "nab2", era: "gcw" }, { map: "geo1", era: "cw" }]);
  available.select(2, ia); available.activate(0, ia);
  assert.equal(ia.entries.length, 2);
  ia.widget("selected").activate(0, ia);
  assert.equal(ia.entries.length, 1);
  ia.widget("launch").activate(0, ia);
  const url = new URL(visits[0], "http://localhost");
  assert.equal(url.searchParams.get("map"), "nab2");
  assert.equal(url.searchParams.get("rotation"), "nab2:gcw");
  assert.equal(url.searchParams.get("noreload"), "1");
  assert.equal(url.searchParams.has("screen"), false);
  ia.widget("order").activate(0, ia);
  ia.widget("launch").activate(0, ia);
  assert.equal(new URL(visits[1], "http://localhost").searchParams.get("order"), "random");
});

test("era cancellation, full playlist and exit confirmation leave state intact", () => {
  const { stack, screens, visits } = fixture();
  choose(stack, "quit");
  assert.equal(stack.top.focused.item, 1);
  stack.key("Enter");
  assert.deepEqual(visits, []);
  stack.push(screens.ia());
  const ia = stack.top;
  ia.widget("available").activate(0, ia);
  stack.key("Escape");
  assert.equal(ia.entries.length, 0);
  for (let i = 0; i < MAX_PLAYLIST; i++) ia.entries.push({ map: "geo1", era: "cw" });
  ia.widget("available").activate(0, ia);
  assert.equal(stack.top.id, "ok");
  assert.equal(ia.entries.length, MAX_PLAYLIST);
});


test("LAN browser launches the advertised world without online service requests", async () => {
  const calls = [];
  globalThis.bfeDesktop = { network: { request: async action => {
    calls.push(action);
    if (action === "notice") return { accepted: false };
    if (action === "discover") return [{ id: "a".repeat(64), name: "Local battle", players: 1, maxPlayers: 16, query: "?map=nab2&side=rep&ai=classic&seed=1", address: "192.168.1.10", port: 18451 }];
    throw Error("Unexpected online request");
  } } };
  try {
    const { stack, visits } = fixture();
    choose(stack, "mp"); choose(stack, "lan");
    await new Promise(resolve => setTimeout(resolve, 0));
    choose(stack, "a".repeat(64));
    await new Promise(resolve => setTimeout(resolve, 0));
    const url = new URL(visits[0], "http://localhost");
    assert.equal(url.searchParams.get("address"), "192.168.1.10");
    assert.equal(url.searchParams.get("seed"), "1");
    assert.equal(url.searchParams.get("network"), "1");
    assert.equal(url.searchParams.get("traceInput"), "1");
    assert.equal(url.searchParams.get("profileRuntime"), "1");
    assert.deepEqual(calls, ["notice", "discover"]);
  } finally { delete globalThis.bfeDesktop; }
});

test("joining a listed server takes the map and side from its mission, never a pick or playlist left in the menu's URL", async () => {
  const visits = [];
  const server = { id: "a".repeat(64), name: "Garage", players: 1, maxPlayers: 8, address: "10.0.0.2", port: 18451, rotation: ["swbf1/nab2:rep:conquest"], relayPolicy: "never" };
  const previous = globalThis.bfeDesktop;
  globalThis.bfeDesktop = { network: { request: async (action) => action === "notice" ? { accepted: true, version: "x" } : action === "discover" ? [server] : { servers: [] } } };
  try {
    const data = { strings: {}, maps: [{ id: "nab2", name: "THEED", extracted: true, eras: { cw: {} } }] };
    const screens = createScreens({ data, settings: profile(), media: { preview() {} },
      query: "?screen=main&map=nab2&side=cis&unit=cis_inf_battledroid_inf&spawn=cp3&rotation=nab2:cw,geo1:cw&round=1&seed=7", navigate: (url) => visits.push(url), close() {} });
    const stack = new ScreenStack({ sound: () => {} });
    stack.push(screens.main());
    choose(stack, "mp");
    choose(stack, "lan");
    for (let n = 0; n < 20 && !stack.top.widget("rows").rows.some((row) => row.id === server.id); n++) await new Promise((resolve) => setTimeout(resolve, 5));
    choose(stack, server.id);
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(visits.length, 1);
    const target = new URLSearchParams(visits[0].slice(2));
    assert.equal(target.get("map"), "nab2"); assert.equal(target.get("side"), "rep");
    assert.equal(target.get("network"), "1"); assert.equal(target.get("address"), "10.0.0.2"); assert.equal(target.get("port"), "18451");
    for (const name of ["unit", "spawn", "spawnscreen", "rotation", "round", "order", "screen"]) assert.equal(target.get(name), null, `${name} must not carry into the server's battle`);
  } finally { globalThis.bfeDesktop = previous; }
});

test("multiplayer asks for a profile first and launches nothing without one", async () => {
  // A tester's suggestion: require a profile before multiplayer launch.
  const lan = { id: "a".repeat(64), name: "Local battle", players: 1, maxPlayers: 16, query: "?map=nab2&side=rep&ai=classic&seed=1", address: "192.168.1.10", port: 18451 };
  const previous = globalThis.bfeDesktop;
  globalThis.bfeDesktop = { network: { request: async action => action === "discover" ? [lan] : { accepted: false } } };
  try {
    for (const [answer, launches] of [[null, 0], ["PLAYER 1", 0], ["Kevin", 1]]) {
      const settings = profile(null), asked = [];
      const { stack, visits } = fixture({ settings, askProfile: async (current, options) => { asked.push([current, options.message]); return answer; } });
      choose(stack, "mp"); choose(stack, "lan");
      await new Promise(resolve => setTimeout(resolve, 0));
      choose(stack, lan.id);
      await new Promise(resolve => setTimeout(resolve, 0));
      assert.equal(asked.length, 1, "a new install's PLAYER 1 is no profile");
      assert.match(asked[0][1], /profile/i);
      assert.equal(visits.length, launches, `answer ${answer}`);
      assert.equal(settings.get("playerName"), answer === "Kevin" ? "Kevin" : undefined);
    }
    const asked = [];
    const { stack, visits } = fixture({ askProfile: async () => { asked.push(1); return null; } });
    choose(stack, "mp"); choose(stack, "lan");
    await new Promise(resolve => setTimeout(resolve, 0));
    choose(stack, lan.id);
    await new Promise(resolve => setTimeout(resolve, 0));
    assert.deepEqual([asked.length, visits.length], [0, 1], "a made profile joins at once");
  } finally { globalThis.bfeDesktop = previous; }
});

test('main menu exposes export/import through the narrow desktop bridge', async t => {
  const previous = globalThis.bfeDesktop;
  t.after(() => { if (previous === undefined) delete globalThis.bfeDesktop; else globalThis.bfeDesktop = previous; });
  const calls = [];
  globalThis.bfeDesktop = { contentTransfer: async action => calls.push(action) };
  const { stack } = fixture();
  choose(stack, 'export-content'); choose(stack, 'import-content');
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(calls, ['export', 'import']);
  delete globalThis.bfeDesktop;
  choose(stack, 'export-content');
  assert.equal(stack.top.id, 'ok');
});
