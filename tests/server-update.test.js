// The dedicated server's updater (electron/update/server.js) against real
// signed releases served from a local feed: a manual deploy replaced once
// nobody plays, game data prepared by each release's own tools (and shared
// by releases with the same tools), a failed release rolled back and set
// aside, preparation that keeps failing given up, and the player count read
// carefully. Nothing here touches systemd or a real server.
import assert from "node:assert/strict";
import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { chmodSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, readlinkSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import path from "node:path";
import { after, test } from "node:test";
import { zstdCompressSync } from "node:zlib";
import treeIndex from "../build/tree-index.cjs";
import { buildPayload } from "../build/update-payload.mjs";
import { keyId } from "../electron/update/feed.js";
import { DATA_INPUTS, dataKey, instancesOf, playersOnServer, readUpdateState, runningVersion, serverTree, serverUnit, SERVER_PARTS, units, updateOnce } from "../electron/update/server.js";

const linux = process.platform === "linux";
const temp = mkdtempSync(path.join(tmpdir(), "bfe-server-update-"));
after(() => rmSync(temp, { recursive: true, force: true }));
let serial = 0;
const scratch = (name) => { const dir = path.join(temp, `${name}-${++serial}`); mkdirSync(dir, { recursive: true }); return dir; };

const { publicKey, privateKey } = generateKeyPairSync("ed25519");
const spki = publicKey.export({ format: "der", type: "spki" });
const KEYS = { [keyId(spki)]: spki.toString("base64") };
const envelope = (release) => ({ key: keyId(spki), payload: Buffer.from(JSON.stringify(release)).toString("base64"), signature: sign(null, Buffer.from(JSON.stringify(release)), privateKey).toString("base64") });

function writeTree(dir, files) {
  for (const [name, value] of Object.entries(files)) {
    const file = path.join(dir, ...name.split("/"));
    mkdirSync(path.dirname(file), { recursive: true });
    if (typeof value === "string") writeFileSync(file, value);
    else if (value.link) symlinkSync(value.link, file);
    else { writeFileSync(file, value.data); chmodSync(file, value.mode); }
  }
}

// A fake Python: tools/prepare.py as the updater runs it, counting runs in
// `counter` and failing while `fail` exists.
const python = (counter, fail) => ({ mode: 0o755, data: `#!/bin/sh
echo prepared >> '${counter}'
echo '@@{"event": "step", "key": "map:nab2", "status": "done", "seconds": 1}'
if [ -e '${fail}' ]; then echo '{"failed": ["map:nab2"], "skipped": []}' > "$BFE_CONTENT_DIR/.prepare.json"; exit 0; fi
mkdir -p "$BFE_CONTENT_DIR/maps/nab2" "$BFE_CONTENT_DIR/maps/bes2"
echo '{}' > "$BFE_CONTENT_DIR/maps/nab2/map.json"
echo '{}' > "$BFE_CONTENT_DIR/maps/bes2/map.json"
echo '{"failed": [], "skipped": [], "game": "'"$BFE_GAME_DIR"'"}' > "$BFE_CONTENT_DIR/.prepare.json"
` });

// A client build as CI packs it, with a server's parts in resources/.
function build({ simulation = "sim-1", tools = "tools one", counter, fail, updater = true }) {
  return {
    "battlefront-eternal": { data: "#!/bin/sh\n", mode: 0o755 },
    "resources/app.asar": `client ${simulation}`,
    "resources/host-runtime/engine/package.json": JSON.stringify({ type: "module" }),
    "resources/host-runtime/engine/src/runtime/network-contract.js": `export const NETWORK_CONTRACT = Object.freeze({ simulation: "${simulation}" });\n`,
    "resources/host-runtime/engine/src/runtime/content-hash.js": "export async function contentHash(query, fetcher) { const map = new URLSearchParams(query).get('map'); if (!(await fetcher(`/maps/${map}/map.json`)).ok) throw Error('Missing map content'); return 'hash'; }\n",
    "resources/host-runtime/server/src/main.js": "// the server\n",
    // An updater that answers --status as the real one does, or (broken)
    // one that exits quietly, as preview 283's did run through a link.
    ...(updater ? { "resources/host-runtime/update/server.js": updater === "broken" ? "// does nothing\n" : "console.log(JSON.stringify({ links: {} }));\n" } : {}),
    "resources/tools/prepare.py": `# ${tools}\n`,
    "resources/tools/content-version.json": JSON.stringify({ state: 2, maps: 13, sides: 3, common: 2 }),
    "resources/toolchain/python/bin/python3": python(counter, fail),
    "resources/toolchain/bin/ffmpeg": { data: "#!/bin/sh\n", mode: 0o755 },
  };
}

// Publishes a build as preview <n> on the feed (what CI's publish step does).
async function publish(site, n, files) {
  const dir = scratch("build");
  writeTree(dir, files);
  await treeIndex.default({ appOutDir: dir, electronPlatformName: "linux" });
  const tar = path.join(temp, `payload-${++serial}.tar`), version = `0.1.0-preview.${n}`, release_id = `preview-${n}`;
  await buildPayload({ tree: dir, platform: "linux-x64", out: tar, version });
  const data = readFileSync(tar), entries = new Map();
  for (let offset = 0; offset + 512 <= data.length;) {
    const header = data.subarray(offset, offset + 512);
    if (header.every((byte) => byte === 0)) break;
    const name = header.subarray(0, 100).toString("utf8").replace(/\0.*$/s, ""), size = parseInt(header.subarray(124, 136).toString("utf8").replace(/\0.*$/s, ""), 8);
    entries.set(name, data.subarray(offset + 512, offset + 512 + size));
    offset += 512 + Math.ceil(size / 512) * 512;
  }
  for (const [name, blob] of entries) {
    const match = /^blobs\/([a-f0-9]{64})\.zst$/.exec(name);
    if (!match) continue;
    const file = path.join(site, "updates", "blobs", match[1].slice(0, 2), `${match[1]}.zst`);
    mkdirSync(path.dirname(file), { recursive: true });
    writeFileSync(file, blob);
  }
  const packed = zstdCompressSync(entries.get("update-linux-x64.json"));
  mkdirSync(path.join(site, release_id), { recursive: true });
  writeFileSync(path.join(site, release_id, "update-linux-x64.json.zst"), packed);
  const release = { schema: 1, channel: "test", version, release_id, revision: `rev${n}`, date: "2026-10-20", content: { state: 2, maps: 13, sides: 3, common: 2 }, platforms: { "linux-x64": { manifest: `${release_id}/update-linux-x64.json.zst`, sha256: createHash("sha256").update(packed).digest("hex") } } };
  mkdirSync(path.join(site, "updates"), { recursive: true });
  writeFileSync(path.join(site, "updates", "test.json"), JSON.stringify(envelope(release)));
  return release;
}

async function serve(site) {
  const server = createServer((request, response) => {
    const file = path.join(site, ...decodeURIComponent(new URL(request.url, "http://x").pathname).split("/").filter(Boolean));
    if (!file.startsWith(site) || !existsSync(file) || lstatSync(file).isDirectory()) { response.writeHead(404).end(); return; }
    const data = readFileSync(file);
    response.writeHead(200, { "content-length": data.length }).end(data);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  after(() => server.close());
  return `http://127.0.0.1:${server.address().port}/`;
}

// A server laid out as Planet Lightwave's was before the updater: folders
// of its own from a manual deploy, a rotation, the owned game.
function server() {
  const root = scratch("server"), stateDir = scratch("state"), retail = scratch("retail");
  for (const name of ["engine", "server", "game-data"]) { mkdirSync(path.join(root, name)); writeFileSync(path.join(root, name, "manual"), name); }
  writeFileSync(path.join(stateDir, "config.json"), JSON.stringify({ rotation: ["swbf1/nab2:rep:conquest"] }));
  mkdirSync(path.join(retail, "GameData"));
  return { root, stateDir, retail };
}

function harness({ base, layout, players = 0, healthy = async () => ({ ok: true }), extra = {} }) {
  const calls = [];
  const config = { channel: "test", feed: base, root: layout.root, stateDir: layout.stateDir, retail: layout.retail, user: null, jobs: 1, service: "bfeserver", ...extra };
  const options = { config, keys: KEYS, platform: "linux-x64", control: async (action, service) => { calls.push(`${action} ${service}`); }, healthy, players: async ({ config: instance }) => (typeof players === "function" ? players(instance) : players), reboot: async () => calls.push("reboot") };
  return { calls, config, run: (overrides = {}) => updateOnce({ ...options, ...overrides }) };
}

const target = (root, name) => readlinkSync(path.join(root, name));
const count = (file) => (existsSync(file) ? readFileSync(file, "utf8").split("\n").filter(Boolean).length : 0);

test("a manual deploy is replaced once nobody plays, and releases with the same tools share their game data", { skip: !linux && "POSIX links and shell" }, async () => {
  const site = scratch("site"), base = await serve(site), layout = server(), counter = path.join(temp, `counter-${++serial}`), fail = path.join(temp, "never");
  await publish(site, 1, build({ simulation: "sim-1", counter, fail }));
  let players = 2;
  const { calls, run } = harness({ base, layout, players: () => players });

  const waiting = await run();
  assert.equal(waiting.action, "waiting");
  assert.equal(count(counter), 1, "the game data is prepared while the server still plays");
  assert.deepEqual(calls, [], "nobody is moved off a server with players on it");
  assert.equal(readUpdateState(layout.root).instances.main.pending.version, "0.1.0-preview.1");
  assert.ok(!lstatSync(path.join(layout.root, "engine")).isSymbolicLink());

  players = 0;
  const updated = await run();
  assert.deepEqual([updated.action, updated.version, updated.moved], ["updated", "0.1.0-preview.1", ["main"]]);
  assert.equal(readUpdateState(layout.root).instances.main.previous, null, "it replaced a manual deploy");
  assert.deepEqual(calls, ["stop bfeserver", "start bfeserver"]);
  assert.equal(count(counter), 1, "prepared once");
  assert.equal(target(layout.root, "engine"), path.join(layout.root, "versions", "0.1.0-preview.1", "resources", "host-runtime", "engine"));
  assert.equal(target(layout.root, "update"), path.join(layout.root, "versions", "0.1.0-preview.1", "resources", "host-runtime", "update"));
  const data = target(layout.root, "game-data");
  assert.ok(existsSync(path.join(data, "maps", "nab2", "map.json")) && existsSync(path.join(data, ".bfe-data.json")));
  assert.equal(readFileSync(path.join(layout.root, "engine.manual", "manual"), "utf8"), "engine", "the manual deploy is kept to roll back to");
  assert.equal(JSON.parse(readFileSync(path.join(layout.root, "versions", "0.1.0-preview.1", "resources", "host-runtime", "engine", "engine-revision.json"))).revision, "rev1");
  assert.equal(runningVersion(layout.root), "0.1.0-preview.1");
  assert.equal((await run()).action, "current");

  // The engine changes, the tools do not: the data is shared.
  await publish(site, 2, build({ simulation: "sim-2", counter, fail }));
  assert.equal((await run()).action, "updated");
  assert.equal(count(counter), 1);
  assert.equal(target(layout.root, "game-data"), data);
  assert.ok(!existsSync(path.join(layout.root, "engine.manual")), "two releases on, the manual deploy goes");

  // New tools: new data, beside the data the previous release uses.
  await publish(site, 3, build({ simulation: "sim-2", tools: "tools two", counter, fail }));
  assert.equal((await run()).action, "updated");
  assert.equal(count(counter), 2);
  assert.notEqual(target(layout.root, "game-data"), data);
  assert.ok(existsSync(data), "the previous release's data stays while it may be rolled back to");
  assert.ok(!existsSync(path.join(layout.root, "versions", "0.1.0-preview.1")), "older versions go");
  assert.ok(existsSync(path.join(layout.root, "versions", "0.1.0-preview.2")));
});

test("a release that does not come up healthy is rolled back and set aside", { skip: !linux && "POSIX links and shell" }, async () => {
  const site = scratch("site"), base = await serve(site), layout = server(), counter = path.join(temp, `counter-${++serial}`), fail = path.join(temp, "never");
  await publish(site, 1, build({ counter, fail }));
  let broken = false;
  const { calls, run } = harness({ base, layout, healthy: async () => { if (broken) throw Error("The server did not become healthy"); return { ok: true }; } });
  await run();
  const before = ["engine", "server", "game-data", "update"].map((name) => target(layout.root, name));

  broken = true;
  await publish(site, 2, build({ simulation: "sim-2", counter, fail }));
  await assert.rejects(run(), /did not become healthy/);
  assert.deepEqual(["engine", "server", "game-data", "update"].map((name) => target(layout.root, name)), before, "every link points where it did");
  assert.deepEqual(calls.slice(-4), ["stop bfeserver", "start bfeserver", "stop bfeserver", "start bfeserver"]);
  assert.deepEqual(readUpdateState(layout.root).bad, ["0.1.0-preview.2"]);
  calls.length = 0;
  assert.equal((await run()).action, "bad", "not tried again");
  assert.deepEqual(calls, []);

  // A manual deploy's folders come back too, when the first release fails.
  const manual = server();
  const first = harness({ base, layout: manual, healthy: async () => { throw Error("The server did not become healthy"); } });
  await assert.rejects(first.run(), /healthy/);
  for (const name of ["engine", "server", "game-data"]) assert.equal(readFileSync(path.join(manual.root, name, "manual"), "utf8"), name);
});

test("preparation that keeps failing is given up until a newer release, and the server is never stopped for it", { skip: !linux && "POSIX links and shell" }, async () => {
  const site = scratch("site"), base = await serve(site), layout = server(), counter = path.join(temp, `counter-${++serial}`), fail = path.join(temp, `fail-${serial}`);
  writeFileSync(fail, "");
  await publish(site, 1, build({ counter, fail }));
  const { calls, run } = harness({ base, layout });
  for (let attempt = 1; attempt <= 3; attempt++) await assert.rejects(run(), /Preparation failed.*map:nab2/);
  await assert.rejects(run(), /failed 3 times/);
  assert.equal(count(counter), 3);
  assert.deepEqual(calls, []);
  assert.equal(readdirSafe(path.join(layout.root, "data")).filter((name) => name.startsWith(".staging")).length, 0, "no half-prepared data is left");

  rmSync(fail);
  await publish(site, 2, build({ tools: "fixed tools", counter, fail }));
  assert.equal((await run()).action, "updated");
});

test("no game to prepare from stops the update before the server is touched", { skip: !linux && "POSIX links and shell" }, async () => {
  const site = scratch("site"), base = await serve(site), layout = server();
  rmSync(path.join(layout.retail, "GameData"), { recursive: true });
  await publish(site, 1, build({ counter: path.join(temp, `counter-${++serial}`), fail: path.join(temp, "never") }));
  const { calls, run } = harness({ base, layout });
  await assert.rejects(run(), /No owned game to prepare from/);
  assert.deepEqual(calls, []);
  assert.match(readUpdateState(layout.root).lastError.message, /No owned game/);
});

test("a release without a working updater keeps the updater already there", { skip: !linux && "POSIX links and shell" }, async () => {
  for (const updater of [false, "broken"]) {
    const site = scratch("site"), base = await serve(site), layout = server(), bootstrap = scratch("bootstrap");
    symlinkSync(bootstrap, path.join(layout.root, "update"));
    await publish(site, 1, build({ counter: path.join(temp, `counter-${++serial}`), fail: path.join(temp, "never"), updater }));
    assert.equal((await harness({ base, layout }).run()).action, "updated");
    assert.equal(target(layout.root, "update"), bootstrap, `${updater || "no"} updater`);
    assert.equal(runningVersion(layout.root), "0.1.0-preview.1", "the release itself still runs");
  }
});

test("the player count: a number from the game port, none from a server between battles, nobody on a server that is down", async () => {
  const config = { info: "http://info/", health: "http://health/" };
  const respond = (routes) => async (url) => {
    const route = routes[url];
    if (!route) throw Error("unreachable");
    return new Response(JSON.stringify(route));
  };
  assert.equal(await playersOnServer({ config, fetchImpl: respond({ "http://info/": { players: 3 } }), pauseMs: 0 }), 3);
  assert.equal(await playersOnServer({ config, fetchImpl: respond({ "http://info/": { players: 0 } }), pauseMs: 0 }), 0);
  assert.equal(await playersOnServer({ config, fetchImpl: respond({ "http://health/": { status: "loading" } }), pauseMs: 0 }), null, "the next battle loading: players may be waiting");
  assert.equal(await playersOnServer({ config, fetchImpl: respond({ "http://health/": { status: "fault" } }), pauseMs: 0 }), 0);
  assert.equal(await playersOnServer({ config, fetchImpl: respond({}), pauseMs: 0 }), 0, "nothing answers: the server is down");
});

test("a server takes the host runtime, tools and toolchain of a release, and its data follows only the tools", () => {
  const tree = { schema: 1, platform: "linux-x64", version: "0.1.0-preview.1", executable: "battlefront-eternal", files: [
    { path: "battlefront-eternal", sha256: "a".repeat(64) },
    { path: "resources/app.asar", sha256: "b".repeat(64) },
    { path: "resources/host-runtime/server/src/main.js", sha256: "c".repeat(64) },
    { path: "resources/host-runtime/engine/src/game.js", sha256: "d".repeat(64) },
    { path: "resources/tools/prepare.py", sha256: "e".repeat(64) },
    { path: "resources/toolchain/python/bin/python3.13", sha256: "f".repeat(64) },
    { path: "resources/toolchain/bin/ffmpeg", sha256: "1".repeat(64) },
  ], links: [{ path: "resources/toolchain/python/bin/python3", target: "python3.13" }, { path: "lib/x", target: "y" }], dirs: ["resources/tools/empty", "locales"] };
  const cut = serverTree(tree);
  assert.deepEqual(cut.files.map((file) => file.path), tree.files.slice(2).map((file) => file.path));
  assert.equal(cut.executable, "resources/host-runtime/server/src/main.js");
  assert.deepEqual(cut.links.map((link) => link.path), ["resources/toolchain/python/bin/python3"]);
  assert.deepEqual(cut.dirs, ["resources/tools/empty"]);
  const changed = (index, sha) => ({ ...cut, files: cut.files.map((file, i) => (i === index ? { ...file, sha256: sha } : file)) });
  assert.equal(dataKey(changed(1, "9".repeat(64))), dataKey(cut), "an engine change keeps the data");
  assert.equal(dataKey(changed(4, "9".repeat(64))), dataKey(cut), "ffmpeg plays no part in gameplay data");
  assert.notEqual(dataKey(changed(2, "9".repeat(64))), dataKey(cut), "a tools change prepares again");
  assert.notEqual(dataKey(changed(3, "9".repeat(64))), dataKey(cut), "so does another Python");
  assert.ok(DATA_INPUTS.every((part) => SERVER_PARTS.some((kept) => part.startsWith(kept))));
  assert.throws(() => serverTree({ ...tree, files: tree.files.slice(0, 2) }), /no server runtime/);
});

test("every release ships the updater with every module it imports, and its timer runs the current one", () => {
  const listed = /const SERVER_UPDATER = \[([^\]]+)\]/.exec(readFileSync(new URL("../build/prepare-host-runtime.mjs", import.meta.url), "utf8"))[1].match(/'([^']+)'/g).map((name) => name.slice(1, -1));
  const needed = new Set(), visit = (name) => {
    if (needed.has(name)) return;
    needed.add(name);
    const text = readFileSync(new URL(`../electron/update/${name}`, import.meta.url), "utf8");
    for (const match of text.matchAll(/from "\.\/([^"]+)"/g)) visit(match[1]);
  };
  visit("server.js");
  assert.deepEqual([...needed].sort(), [...listed].sort());
  const { "bfeserver-update.service": service, "bfeserver-update.timer": timer } = units({ root: "/opt/bfeserver", node: "/opt/bfeserver/node/bin/node" });
  assert.match(service, /ExecStart=\/opt\/bfeserver\/node\/bin\/node \/opt\/bfeserver\/update\/server\.js --config=\/etc\/bfeserver-update\.json/);
  assert.match(service, /Nice=19/);
  assert.match(timer, /OnUnitInactiveSec=5min/);
});

test("the updater runs as a program through the link a server starts it by", { skip: !linux && "POSIX links" }, async () => {
  const { execFileSync } = await import("node:child_process");
  const root = scratch("cli"), link = path.join(root, "update");
  symlinkSync(path.resolve("electron/update"), link);
  writeFileSync(path.join(root, "config.json"), JSON.stringify({ root }));
  const status = JSON.parse(execFileSync(process.execPath, [path.join(link, "server.js"), "--status", `--config=${path.join(root, "config.json")}`], { encoding: "utf8" }));
  assert.equal(status.running, null);
  assert.equal(status.links.update, path.resolve("electron/update"));
});

test("a second server on the machine is set up and started on the release while the first still has players, then the first follows once empty", { skip: !linux && "POSIX links and shell" }, async () => {
  const site = scratch("site"), base = await serve(site), layout = server(), counter = path.join(temp, `counter-${++serial}`), fail = path.join(temp, "never");
  const duelState = scratch("duel-state");
  writeFileSync(path.join(duelState, "config.json"), JSON.stringify({ rotation: ["swbf1/bes2:rep:conquest", "swbf1/bes2:imp:conquest"], ai: "off" }));
  const instances = [{ name: "main" }, { name: "produals", stateDir: duelState, healthPort: 18453, gamePort: 18452 }];
  await publish(site, 1, build({ counter, fail }));
  const playing = { main: 3, produals: 0 };
  const { calls, run } = harness({ base, layout, players: (instance) => playing[instance.name], extra: { instances } });

  const first = await run();
  assert.deepEqual([first.action, first.moved, first.waiting], ["updated", ["produals"], ["main"]]);
  assert.deepEqual(calls, ["stop bfeserver-produals", "start bfeserver-produals"], "the busy server is left alone");
  const duelLinks = path.join(layout.root, "instances", "produals");
  assert.equal(target(duelLinks, "engine"), path.join(layout.root, "versions", "0.1.0-preview.1", "resources", "host-runtime", "engine"));
  assert.equal(runningVersion(layout.root, duelLinks), "0.1.0-preview.1");
  assert.ok(!lstatSync(path.join(layout.root, "engine")).isSymbolicLink(), "the main server still runs its manual deploy");
  assert.equal(readUpdateState(layout.root).instances.main.pending.version, "0.1.0-preview.1");

  playing.main = 0;
  calls.length = 0;
  const second = await run();
  assert.deepEqual([second.action, second.moved], ["updated", ["main"]]);
  assert.deepEqual(calls, ["stop bfeserver", "start bfeserver"]);
  assert.equal(target(layout.root, "game-data"), target(duelLinks, "game-data"), "one release, one copy of its data");
  assert.equal(count(counter), 1);

  // A newer release while the duel server is busy: main moves, the duel
  // server keeps the version it runs, which stays installed.
  await publish(site, 2, build({ simulation: "sim-2", counter, fail }));
  playing.produals = 2;
  calls.length = 0;
  assert.deepEqual((await run()).moved, ["main"]);
  assert.equal(runningVersion(layout.root, duelLinks), "0.1.0-preview.1");
  assert.ok(existsSync(path.join(layout.root, "versions", "0.1.0-preview.1")), "a version a server runs is kept");
});

test("a server without its configuration is not started, and the release is not set aside for it", { skip: !linux && "POSIX links and shell" }, async () => {
  const site = scratch("site"), base = await serve(site), layout = server();
  await publish(site, 1, build({ counter: path.join(temp, `counter-${++serial}`), fail: path.join(temp, "never") }));
  const instances = [{ name: "main" }, { name: "new", stateDir: scratch("empty-state"), healthPort: 18463, gamePort: 18462 }];
  const { calls, run } = harness({ base, layout, extra: { instances } });
  const result = await run();
  assert.deepEqual([result.moved, result.waiting], [["main"], ["new"]]);
  assert.ok(!calls.some((call) => call.includes("bfeserver-new")));
  assert.deepEqual(readUpdateState(layout.root).bad, []);
});

test("the machine restarts for its updates only once every server is empty, and not again for six hours", { skip: !linux && "POSIX links and shell" }, async () => {
  const site = scratch("site"), base = await serve(site), layout = server(), flag = path.join(scratch("run"), "reboot-required");
  await publish(site, 1, build({ counter: path.join(temp, `counter-${++serial}`), fail: path.join(temp, "never") }));
  let players = 1;
  const { calls, run } = harness({ base, layout, players: () => players, extra: { rebootWhenRequired: true, rebootFlag: flag } });
  await run();
  writeFileSync(flag, "*** System restart required ***\n");
  writeFileSync(`${flag}.pkgs`, "linux-image-generic\nlibc6\n");
  calls.length = 0;
  await run();
  assert.ok(!calls.includes("reboot"), "players are on");
  players = 0;
  await run();
  assert.deepEqual(calls.filter((call) => call === "reboot"), ["reboot"]);
  assert.ok(readUpdateState(layout.root).lastReboot);
  await run();
  assert.deepEqual(calls.filter((call) => call === "reboot"), ["reboot"], "not twice in six hours");
  const off = harness({ base, layout: server(), extra: { rebootFlag: flag } });
  await off.run();
  assert.ok(!off.calls.includes("reboot"), "only with rebootWhenRequired");
});

test("servers: the main one as the configuration names it, the others by name, and their units", () => {
  assert.deepEqual(instancesOf({ ...DEFAULT_LIKE }).map((instance) => [instance.name, instance.service, instance.links, instance.unit]), [["main", "bfeserver", "/opt/bfeserver", false]]);
  const [main, duel] = instancesOf({ ...DEFAULT_LIKE, instances: [{ name: "main" }, { name: "produals", healthPort: 18453, gamePort: 18452 }] });
  assert.equal(main.info, "http://127.0.0.1:18451/info");
  assert.deepEqual([duel.service, duel.links, duel.stateDir, duel.health, duel.info, duel.unit], ["bfeserver-produals", "/opt/bfeserver/instances/produals", "/var/lib/bfeserver-produals", "http://127.0.0.1:18453/healthz", "http://127.0.0.1:18452/info", true]);
  assert.throws(() => instancesOf({ ...DEFAULT_LIKE, instances: [{ name: "a", healthPort: 1, gamePort: 2 }, { name: "a", healthPort: 3, gamePort: 4 }] }), /repeated/);
  assert.throws(() => instancesOf({ ...DEFAULT_LIKE, instances: [{ name: "nopor" }] }), /needs healthPort/);
  const unit = serverUnit({ instance: duel, node: "/opt/bfeserver/node/bin/node", user: "bfeserver" });
  for (const line of ["Environment=BFE_STATE_DIR=/var/lib/bfeserver-produals", "Environment=PORT=18453", "ExecStart=/opt/bfeserver/node/bin/node /opt/bfeserver/instances/produals/server/src/main.js --serve", "Restart=always", "ReadWritePaths=/var/lib/bfeserver-produals", "MemoryMax=1500M", "WantedBy=multi-user.target"]) assert.ok(unit.includes(line), line);
  assert.deepEqual(Object.keys(units({ instances: [main, duel] })).sort(), ["bfeserver-produals.service", "bfeserver-update.service", "bfeserver-update.timer"], "the main server's own unit is the operator's");
});

test("a state from before servers had names describes the main one", () => {
  const root = scratch("legacy");
  writeFileSync(path.join(root, "update-state.json"), JSON.stringify({ current: "0.1.0-preview.284", previous: "0.1.0-preview.283", bad: ["0.1.0-preview.9"], updatedAt: "t" }));
  const state = readUpdateState(root);
  assert.deepEqual(state.instances.main, { current: "0.1.0-preview.284", previous: "0.1.0-preview.283", pending: undefined, updatedAt: "t" });
  assert.equal(state.current, undefined);
  assert.deepEqual(state.bad, ["0.1.0-preview.9"]);
});

const DEFAULT_LIKE = { root: "/opt/bfeserver", service: "bfeserver", stateDir: "/var/lib/bfeserver", health: "http://127.0.0.1:18450/healthz", info: "http://127.0.0.1:18451/info", memoryMax: "1500M", healthTimeoutMs: 1000 };

function readdirSafe(dir) {
  try { return readdirSync(dir); } catch { return []; }
}
