import assert from "node:assert/strict";
import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { chmodSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, readlinkSync, rmSync, statSync, symlinkSync, utimesSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import path from "node:path";
import { after, test } from "node:test";
import { zstdCompressSync } from "node:zlib";
import treeIndex from "../build/tree-index.cjs";
import { buildPayload } from "../build/update-payload.mjs";
import { fetchRelease, fetchTree, keyId, openEnvelope } from "../electron/update/feed.js";
import { createUpdater, handOff } from "../electron/update/index.js";
import { installRelease } from "../electron/update/install.js";
import { childEnvironment, launchVersion, signalReady } from "../electron/update/launch.js";
import { changeState, chooseLaunch, collect, INSTALL_RECORD, installedVersions, MAX_ATTEMPTS, pointCurrent, readState, recordFailure, recordHealthy } from "../electron/update/state.js";
import { hashFile, linkTarget, readTreeIndex, safeRelative, validateTree, walkTree } from "../electron/update/tree.js";
import { compareVersions, isNewer, parseVersion } from "../electron/update/version.js";
import { adoptInstallerLauncher, launcherScript, updatingLauncher } from "../electron/linux-integration.js";

const temp = mkdtempSync(path.join(tmpdir(), "bfe-update-"));
after(() => rmSync(temp, { recursive: true, force: true }));
let serial = 0;
const scratch = (name) => { const dir = path.join(temp, `${name}-${++serial}`); mkdirSync(dir, { recursive: true }); return dir; };
const linux = process.platform === "linux";

// ---- A miniature release pipeline: packaged trees, payloads, a feed ----

const { publicKey, privateKey } = generateKeyPairSync("ed25519");
const spki = publicKey.export({ format: "der", type: "spki" });
const KEYS = { [keyId(spki)]: spki.toString("base64") };
const envelope = (release, key = privateKey) => ({ key: keyId(spki), payload: Buffer.from(JSON.stringify(release)).toString("base64"), signature: sign(null, Buffer.from(JSON.stringify(release)), key).toString("base64") });

function writeTree(dir, files) {
  for (const [name, value] of Object.entries(files)) {
    if (value === undefined) continue;
    const file = path.join(dir, ...name.split("/"));
    mkdirSync(path.dirname(file), { recursive: true });
    if (typeof value === "string") writeFileSync(file, value);
    else if (value.link) symlinkSync(value.link, file);
    else {
      writeFileSync(file, value.data);
      chmodSync(file, value.mode);
    }
  }
}

const BASE_FILES = {
  "battlefront-eternal": { data: "#!/bin/sh\necho v1\n", mode: 0o755 },
  "chrome-sandbox": { data: "setuid helper", mode: 0o755 },
  "libffmpeg.so": "codec ".repeat(5000),
  "resources/app.asar": "app version one ".repeat(4000),
  "resources/tools/content-version.json": JSON.stringify({ state: 2, maps: 8, sides: 2 }),
  "resources/toolchain/python/bin/python3.13": { data: "python", mode: 0o755 },
  "resources/toolchain/python/bin/python3": { link: "python3.13" },
  "resources/toolchain/python/share/terminfo/E/Eterm": "upper",
  "resources/toolchain/python/share/terminfo/e/eterm": "lower",
  "locales/en-US.pak": "english",
};

async function packagedTree(files) {
  const dir = scratch("tree");
  writeTree(dir, files);
  await treeIndex.default({ appOutDir: dir, electronPlatformName: "linux" });
  return dir;
}

// A ustar reader for the payload builder's archives.
function readTar(file) {
  const data = readFileSync(file), entries = new Map();
  for (let offset = 0; offset + 512 <= data.length;) {
    const header = data.subarray(offset, offset + 512);
    if (header.every((byte) => byte === 0)) break;
    const name = header.subarray(0, 100).toString("utf8").replace(/\0.*$/s, "");
    const size = parseInt(header.subarray(124, 136).toString("utf8").replace(/\0.*$/s, ""), 8);
    entries.set(name, data.subarray(offset + 512, offset + 512 + size));
    offset += 512 + Math.ceil(size / 512) * 512;
  }
  return entries;
}

// What build/publish-distribution.py does with a payload, enough to serve
// it: blobs into the store, the manifest into the release, a signed channel.
function publishFeed(site, { tar, platform, version, release_id, channel = "test-channel", content }) {
  const entries = readTar(tar);
  for (const [name, data] of entries) {
    const match = /^blobs\/([a-f0-9]{64})\.zst$/.exec(name);
    if (!match) continue;
    const blob = path.join(site, "updates", "blobs", match[1].slice(0, 2), `${match[1]}.zst`);
    mkdirSync(path.dirname(blob), { recursive: true });
    writeFileSync(blob, data);
  }
  const packed = zstdCompressSync(entries.get(`update-${platform}.json`));
  mkdirSync(path.join(site, release_id), { recursive: true });
  writeFileSync(path.join(site, release_id, `update-${platform}.json.zst`), packed);
  const release = { schema: 1, channel, version, release_id, revision: "r", date: "2026-10-17", content, platforms: { [platform]: { manifest: `${release_id}/update-${platform}.json.zst`, sha256: createHash("sha256").update(packed).digest("hex") } } };
  mkdirSync(path.join(site, "updates"), { recursive: true });
  writeFileSync(path.join(site, "updates", `${channel}.json`), JSON.stringify(envelope(release)));
  return release;
}

async function serve(site, { tamper } = {}) {
  const requests = [];
  const server = createServer((request, response) => {
    const url = decodeURIComponent(new URL(request.url, "http://x").pathname);
    requests.push(url);
    const file = path.join(site, ...url.split("/").filter(Boolean));
    if (!file.startsWith(site) || !existsSync(file) || statSync(file).isDirectory()) { response.writeHead(404).end(); return; }
    let data = readFileSync(file);
    if (tamper?.(url)) data = zstdCompressSync(Buffer.from("not what was signed"));
    response.writeHead(200, { "content-length": data.length }).end(data);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  after(() => server.close());
  return { base: `http://127.0.0.1:${server.address().port}/`, requests };
}

const blobRequests = (requests) => requests.filter((url) => url.startsWith("/updates/blobs/"));

// ---- Tests ----

test("versions order previews within a base version, then the release", () => {
  assert.deepEqual(parseVersion("0.1.0-preview.262"), [0, 1, 0, 262]);
  assert.deepEqual(parseVersion("0.1.0~preview.262"), [0, 1, 0, 262]);
  assert.equal(parseVersion("latest"), null);
  assert.ok(isNewer("0.1.0-preview.262", "0.1.0-preview.99"));
  assert.ok(isNewer("0.1.0", "0.1.0-preview.999"));
  assert.ok(isNewer("0.2.0-preview.1", "0.1.0"));
  assert.equal(compareVersions("0.1.0~preview.5", "0.1.0-preview.5"), 0);
  assert.throws(() => compareVersions("nonsense", "0.1.0"));
});

test("a manifest's paths and links stay inside the version folder", () => {
  for (const bad of ["", "/etc/passwd", "../up", "a/../../b", "a//b", "a/./b", "C:/x", "a\\b", "x\u0000y"]) assert.equal(safeRelative(bad), false, bad);
  assert.equal(safeRelative("resources/toolchain/python/lib/python3.13/os.py"), true);
  assert.equal(linkTarget("share/terminfo/a/ansi", "../x/xterm"), "share/terminfo/x/xterm");
  assert.equal(linkTarget("bin/python3", "../../../etc/passwd"), null);
  assert.equal(linkTarget("bin/python3", "/usr/bin/python3"), null);
  const file = (name) => ({ path: name, size: 1, sha256: "a".repeat(64), zsize: 1, mode: 0o644 });
  const tree = (platform, files) => ({ schema: 1, platform, version: "0.1.0-preview.2", executable: "app", files: [{ ...file("app"), mode: 0o755 }, ...files] });
  // Linux folders hold names that differ only by case; Windows ones cannot.
  assert.doesNotThrow(() => validateTree(tree("linux-x64", [file("E/Eterm"), file("e/eterm")])));
  assert.throws(() => validateTree(tree("win32-x64", [file("E/Eterm"), file("e/eterm")])), /twice/);
  assert.throws(() => validateTree(tree("linux-x64", [file("../escape")])), /invalid file/);
  assert.throws(() => validateTree({ ...tree("linux-x64", []), links: [{ path: "x", target: "../../out" }] }), /invalid link/);
  assert.throws(() => validateTree({ ...tree("linux-x64", []), executable: "missing" }), /no executable/);
  assert.throws(() => validateTree(tree("linux-x64", []), { version: "0.1.0-preview.3" }), /another build/);
});

test("only a release signed by a shipped key is believed", async () => {
  const release = { schema: 1, channel: "test-channel", version: "0.1.0-preview.9", release_id: "preview-9", platforms: { "linux-x64": { manifest: "preview-9/update-linux-x64.json.zst", sha256: "b".repeat(64) } } };
  assert.equal(openEnvelope(envelope(release), KEYS).version, "0.1.0-preview.9");
  const forged = envelope(release, generateKeyPairSync("ed25519").privateKey);
  assert.throws(() => openEnvelope(forged, KEYS), /signature is invalid/);
  const altered = { ...envelope(release), payload: Buffer.from(JSON.stringify({ ...release, version: "0.1.0-preview.999" })).toString("base64") };
  assert.throws(() => openEnvelope(altered, KEYS), /signature is invalid/);
  assert.throws(() => openEnvelope({ ...envelope(release), key: "0123456789abcdef" }, KEYS), /trusted key/);
  assert.throws(() => openEnvelope({ ...envelope(release), key: "__proto__" }, KEYS), /trusted key/);
  assert.throws(() => openEnvelope(envelope({ ...release, platforms: { "linux-x64": { manifest: "../../etc", sha256: "b".repeat(64) } } }), KEYS), /malformed/);
  const reply = (body, status = 200) => async () => new Response(typeof body === "string" ? body : JSON.stringify(body), { status });
  assert.equal(await fetchRelease({ channel: "test-channel", keys: KEYS, fetch: reply("", 404) }), null, "a channel with no releases yet");
  await assert.rejects(fetchRelease({ channel: "other", keys: KEYS, fetch: reply(envelope(release)) }), /another channel/);
  await assert.rejects(fetchRelease({ channel: "test-channel", keys: KEYS, fetch: reply("x".repeat(2 << 20)) }), /more than expected/);
});

test("an update downloads only changed files and reuses the rest from the installed build", async () => {
  const one = await packagedTree(BASE_FILES);
  const two = await packagedTree({ ...BASE_FILES, "resources/app.asar": "app version two ".repeat(4000), "resources/new.txt": "added",
    // Moved, unchanged: reused by its hash under the new name.
    "locales/en-US.pak": undefined, "locales/en-GB.pak": "english" });
  const site = scratch("site");
  const tarOne = path.join(temp, `one-${serial}.tar`), tarTwo = path.join(temp, `two-${serial}.tar`);
  const first = await buildPayload({ tree: one, platform: "linux-x64", out: tarOne, version: "0.1.0-preview.1" });
  assert.equal(first.files, Object.keys(BASE_FILES).length - 2 + 1, "chrome-sandbox stays out, the link is a link, the build's own index is a file");
  assert.equal(first.links, 1);
  publishFeed(site, { tar: tarOne, platform: "linux-x64", version: "0.1.0-preview.1", release_id: "preview-1" });
  const known = {};
  for (const name of readTar(tarOne).keys()) if (name.startsWith("blobs/")) known[name.slice(6, 70)] = statSync(path.join(site, "updates", "blobs", name.slice(6, 8), name.slice(6))).size;
  const second = await buildPayload({ tree: two, platform: "linux-x64", out: tarTwo, version: "0.1.0-preview.2", known });
  assert.equal(second.fresh, 3, "app.asar, new.txt and the new index: the server holds every other blob");
  const release = publishFeed(site, { tar: tarTwo, platform: "linux-x64", version: "0.1.0-preview.2", release_id: "preview-2", content: { state: 2, maps: 9, sides: 2 } });
  const { base, requests } = await serve(site);

  const found = await fetchRelease({ channel: "test-channel", keys: KEYS, fetch, base });
  const tree = await fetchTree({ release: found, platform: "linux-x64", fetch, base });
  assert.equal(found.content.maps, 9, "the signed release names its content generations");
  assert.equal(tree.content.maps, 8, "as read from the build's own content-version.json");
  const root = scratch("root"), progress = [];
  const installed = await installRelease({ root, release: found, tree, sources: [one], fetch, base, onProgress: (item) => progress.push(item), platform: "linux" });
  assert.equal(installed.version, "0.1.0-preview.2");
  assert.equal(blobRequests(requests).length, 3);
  assert.ok(progress.some((item) => item.phase === "downloading" && item.received === item.total && item.total > 0));
  for (const file of tree.files) {
    const target = path.join(installed.dir, ...file.path.split("/"));
    assert.equal(await hashFile(target), file.sha256, file.path);
    if (linux) assert.equal(statSync(target).mode & 0o777, file.mode, file.path);
  }
  if (linux) {
    assert.equal(readlinkSync(path.join(installed.dir, "resources/toolchain/python/bin/python3")), "python3.13");
    assert.equal(readFileSync(path.join(installed.dir, "resources/toolchain/python/share/terminfo/E/Eterm"), "utf8"), "upper");
  }
  assert.equal(existsSync(path.join(installed.dir, "chrome-sandbox")), false);
  assert.equal(existsSync(path.join(installed.dir, "locales", "en-US.pak")), false, "removed files stay removed");
  assert.deepEqual(installedVersions(root).map((item) => item.version), ["0.1.0-preview.2"]);
  // The installed version carries its build's index for the next update.
  const index = new Map((await readTreeIndex(installed.dir)).map((file) => [file.path, file.sha256]));
  for (const file of tree.files) if (file.path !== "resources/bfe-tree.json") assert.equal(index.get(file.path), file.sha256, file.path);
  const again = await installRelease({ root, release: found, tree, sources: [one], fetch, base, platform: "linux" });
  assert.equal(again.downloaded, 0, "an installed version is not fetched again");
  assert.equal(release.platforms["linux-x64"].sha256.length, 64);
});

test("a payload refuses a build whose index describes a file otherwise", async () => {
  const tree = await packagedTree(BASE_FILES), out = path.join(temp, `index-${serial}.tar`);
  // An installer target adding a file after packing is fine.
  writeFileSync(path.join(tree, "resources", "elevate.exe"), "added by the installer target");
  assert.equal((await buildPayload({ tree, platform: "linux-x64", out, version: "0.1.0-preview.4" })).files, 10);
  writeFileSync(path.join(tree, "resources", "app.asar"), "changed after indexing");
  await assert.rejects(buildPayload({ tree, platform: "linux-x64", out, version: "0.1.0-preview.4" }), /does not match resources\/app.asar/);
});

test("a file that does not match the signed hash, or a cancel, leaves nothing installed", async () => {
  const tree = await packagedTree(BASE_FILES), site = scratch("site"), tar = path.join(temp, `bad-${serial}.tar`);
  await buildPayload({ tree, platform: "linux-x64", out: tar, version: "0.1.0-preview.3" });
  publishFeed(site, { tar, platform: "linux-x64", version: "0.1.0-preview.3", release_id: "preview-3" });
  const target = (await walkTree(tree)).files.find((file) => file.path === "resources/app.asar").sha256;
  const { base } = await serve(site, { tamper: (url) => url.includes(target) });
  const release = await fetchRelease({ channel: "test-channel", keys: KEYS, fetch, base });
  const manifest = await fetchTree({ release, platform: "linux-x64", fetch, base });
  const root = scratch("root");
  await assert.rejects(installRelease({ root, release, tree: manifest, fetch, base, platform: "linux" }), /did not match the signed release/);
  assert.deepEqual(installedVersions(root), []);
  assert.deepEqual(readdir(path.join(root, "versions")), [], "the staging folder is gone");
  const abort = new AbortController();
  const cancelled = installRelease({ root, release, tree: manifest, fetch: async (...args) => { abort.abort(Error("cancelled")); return fetch(...args); }, base, signal: abort.signal, platform: "linux" });
  await assert.rejects(cancelled, /cancelled/);
  assert.deepEqual(readdir(path.join(root, "versions")), []);
});
const readdir = (dir) => { try { return readdirSync(dir); } catch { return []; } };

function fakeInstall(root, version, { executable = "app", script = "exit 0" } = {}) {
  const dir = path.join(root, "versions", version);
  mkdirSync(dir, { recursive: true });
  writeFileSync(path.join(dir, executable), `#!/bin/sh\n${script}\n`, { mode: 0o755 });
  writeFileSync(path.join(dir, INSTALL_RECORD), JSON.stringify({ version, executable }));
  return dir;
}

test("launches skip versions that keep dying and come back to healthy ones", () => {
  const root = scratch("root");
  for (const version of ["0.1.0-preview.4", "0.1.0-preview.5", "0.1.0-preview.6"]) fakeInstall(root, version);
  const installed = installedVersions(root);
  assert.deepEqual(installed.map((item) => item.version), ["0.1.0-preview.6", "0.1.0-preview.5", "0.1.0-preview.4"]);
  assert.equal(chooseLaunch({ own: "0.1.0-preview.5", installed, state: readState(root) }).target.version, "0.1.0-preview.6");
  assert.equal(chooseLaunch({ own: "0.1.0-preview.6", installed, state: readState(root) }).target, null, "never an older version");
  for (let i = 0; i < MAX_ATTEMPTS; i++) recordFailure(root, "0.1.0-preview.6");
  assert.deepEqual(readState(root).bad, ["0.1.0-preview.6"]);
  assert.equal(chooseLaunch({ own: "0.1.0-preview.4", installed, state: readState(root) }).target.version, "0.1.0-preview.5");
  recordHealthy(root, "0.1.0-preview.6");
  assert.deepEqual(readState(root).bad, []);
  assert.equal(readState(root).attempts["0.1.0-preview.6"], undefined);
  // A record-less folder (an interrupted delete) is not an install.
  mkdirSync(path.join(root, "versions", "0.1.0-preview.7"));
  assert.equal(installedVersions(root)[0].version, "0.1.0-preview.6");
});

test("collection keeps what runs, what waits and one fallback, and nothing else", () => {
  const root = scratch("root");
  const dirs = Object.fromEntries(["1", "2", "3", "4", "5", "6"].map((n) => [n, fakeInstall(root, `0.1.0-preview.${n}`)]));
  recordHealthy(root, "0.1.0-preview.2");
  recordHealthy(root, "0.1.0-preview.3");
  changeState(root, (state) => { state.bad.push("0.1.0-preview.6"); });
  mkdirSync(path.join(root, "versions", "0.1.0-preview.9"));
  const stale = path.join(root, "versions", ".staging-0.1.0-preview.8-1-abcd");
  mkdirSync(stale);
  utimesSync(stale, new Date(0), new Date(0));
  const fresh = path.join(root, "versions", ".staging-0.1.0-preview.8-2-abcd");
  mkdirSync(fresh);
  // A hosted server still runs from version 1.
  const removed = collect(root, { own: "0.1.0-preview.4", protect: [path.join(dirs["1"], "resources", "host-runtime", "engine")] });
  assert.deepEqual(removed.sort(), ["0.1.0-preview.2", "0.1.0-preview.6"]);
  assert.deepEqual(installedVersions(root).map((item) => item.version), ["0.1.0-preview.5", "0.1.0-preview.4", "0.1.0-preview.3", "0.1.0-preview.1"]);
  assert.equal(existsSync(path.join(root, "versions", "0.1.0-preview.9")), false, "record-less leftovers go");
  assert.equal(existsSync(stale), false);
  assert.equal(existsSync(fresh), true, "an install in progress stays");
  // The installed package runs: it is its own fallback.
  collect(root, { own: "0.1.0-preview.4", keepFallback: false });
  assert.deepEqual(installedVersions(root).map((item) => item.version), ["0.1.0-preview.5", "0.1.0-preview.4"]);
  if (linux) {
    assert.equal(pointCurrent(root, dirs["5"]), true);
    assert.equal(readlinkSync(path.join(root, "current")), path.join("versions", "0.1.0-preview.5"));
    assert.equal(pointCurrent(root, dirs["5"]), false);
    rmSync(dirs["5"], { recursive: true });
    collect(root, { own: "0.1.0-preview.4" });
    assert.equal(existsSync(path.join(root, "current")) || lstatExists(path.join(root, "current")), false, "a dangling link goes");
  }
});
const lstatExists = (file) => { try { lstatSync(file); return true; } catch { return false; } };

test("a version started from an AppImage keeps none of the AppImage's mount", () => {
  const env = { APPDIR: "/tmp/.mount_bfeX", APPIMAGE: "/home/p/Battlefront.AppImage", ARGV0: "x", OWD: "/home/p", PATH: "/tmp/.mount_bfeX:/tmp/.mount_bfeX/usr/sbin:/usr/bin", LD_LIBRARY_PATH: "/tmp/.mount_bfeX/usr/lib", XDG_DATA_DIRS: "/tmp/.mount_bfeX/usr/share/::/usr/share/", HOME: "/home/p", ELECTRON_RUN_AS_NODE: "1", BFE_LAUNCH_TOKEN: "f".repeat(32) };
  const next = childEnvironment(env, { platform: "linux" });
  assert.equal(next.PATH, "/usr/bin");
  assert.equal(next.LD_LIBRARY_PATH, undefined);
  assert.equal(next.XDG_DATA_DIRS, "/usr/share/");
  assert.equal(next.BFE_APPIMAGE, "/home/p/Battlefront.AppImage");
  for (const name of ["APPDIR", "APPIMAGE", "ARGV0", "OWD", "ELECTRON_RUN_AS_NODE", "BFE_LAUNCH_TOKEN"]) assert.equal(next[name], undefined, name);
  assert.equal(next.HOME, "/home/p");
  // Only a root-owned setuid helper counts (CI runs as root, where the
  // fixture is one).
  const dir = scratch("bootstrap"), helper = path.join(dir, "chrome-sandbox");
  writeFileSync(helper, "x");
  chmodSync(helper, 0o4755);
  assert.equal(childEnvironment({}, { execDir: dir, platform: "linux" }).CHROME_DEVEL_SANDBOX, process.getuid?.() === 0 ? helper : undefined);
  chmodSync(helper, 0o755);
  assert.equal(childEnvironment({}, { execDir: dir, platform: "linux" }).CHROME_DEVEL_SANDBOX, undefined, "not setuid");
});

test("a launch waits for the new window, and notices a version that dies first", { skip: !linux && "shell launchers" }, async () => {
  const root = scratch("root");
  const ready = fakeInstall(root, "0.1.0-preview.20", { script: 'echo $$ > "$BFE_LAUNCH_DIR/$BFE_LAUNCH_TOKEN"; sleep 2' });
  const dies = fakeInstall(root, "0.1.0-preview.21", { script: "exit 3" });
  const result = await launchVersion({ executable: path.join(ready, "app"), env: process.env, root, timeoutMs: 5000 });
  assert.deepEqual(result, { started: true });
  const failed = await launchVersion({ executable: path.join(dies, "app"), env: process.env, root, timeoutMs: 5000 });
  assert.equal(failed.started, false);
  assert.equal(failed.code, 3);
  assert.equal(signalReady({ BFE_LAUNCH_TOKEN: "../escape", BFE_LAUNCH_DIR: root }), false);
});

test("the boot hand-off starts the newest installed version, or carries on when it fails", { skip: !linux && "shell launchers" }, async () => {
  const home = scratch("home"), env = { ...process.env, XDG_DATA_HOME: home };
  delete env.BFE_HANDOFF;
  delete env.BFE_NO_UPDATE;
  const root = path.join(home, "battlefront-eternal");
  const app = { isPackaged: true, getVersion: () => "0.1.0-preview.30" };
  assert.equal(await handOff({ app, env, argv: ["exe"] }), false, "nothing installed");
  fakeInstall(root, "0.1.0-preview.31", { script: 'echo "$BFE_HANDOFF $*" > "$BFE_LAUNCH_DIR/../args"; echo $$ > "$BFE_LAUNCH_DIR/$BFE_LAUNCH_TOKEN"' });
  assert.equal(await handOff({ app, env, argv: ["exe", "--query=map=nab2"] }), true);
  assert.equal(readFileSync(path.join(root, "args"), "utf8").trim(), "1 --query=map=nab2", "the user's arguments pass on; the new version does not hand off again");
  assert.equal(await handOff({ app, env: { ...env, BFE_HANDOFF: "1" }, argv: ["exe"] }), false);
  fakeInstall(root, "0.1.0-preview.32", { script: "exit 1" });
  assert.equal(await handOff({ app, env, argv: ["exe"] }), true, "a dying update falls back to the next newest at once");
  assert.equal(readState(root).attempts["0.1.0-preview.32"], 1);
  assert.equal(await handOff({ app, env, argv: ["exe"] }), true);
  assert.deepEqual(readState(root).bad, ["0.1.0-preview.32"], "and is set aside after a second failure");
  rmSync(path.join(root, "versions", "0.1.0-preview.31"), { recursive: true });
  fakeInstall(root, "0.1.0-preview.33", { script: "exit 1" });
  assert.equal(await handOff({ app, env, argv: ["exe"] }), false, "with nothing else working, this executable runs");
  assert.equal(await handOff({ app: { ...app, isPackaged: false }, env, argv: ["exe"] }), false, "development runs never hand off");
});

test("started by Steam, the hand-off stays until the version it started exits, and passes on its exit code", { skip: !linux && "shell launchers" }, async () => {
  const home = scratch("home"), env = { ...process.env, XDG_DATA_HOME: home };
  delete env.BFE_HANDOFF;
  delete env.BFE_NO_UPDATE;
  const root = path.join(home, "battlefront-eternal");
  const app = { isPackaged: true, getVersion: () => "0.1.0-preview.40" };
  fakeInstall(root, "0.1.0-preview.41", { script: 'echo "$SteamGameId $BFE_STEAM_PARENT $*" > "$BFE_LAUNCH_DIR/../args"; echo $$ > "$BFE_LAUNCH_DIR/$BFE_LAUNCH_TOKEN"; sleep 0.5; exit 7' });
  const begun = Date.now();
  const result = await handOff({ app, env: { ...env, SteamGameId: "123" }, argv: ["exe", "--steam"], stay: true });
  assert.equal(result.started, true);
  assert.equal(await result.exited, 7);
  assert.ok(Date.now() - begun >= 500, "it waited for the game to end");
  assert.equal(readFileSync(path.join(root, "args"), "utf8").trim(), `123 ${process.pid} --steam`, "Steam's marks pass on, and the process to follow");
  // Without stay, as before: true once the window is up.
  fakeInstall(root, "0.1.0-preview.42", { script: 'echo $$ > "$BFE_LAUNCH_DIR/$BFE_LAUNCH_TOKEN"' });
  assert.equal(await handOff({ app, env, argv: ["exe"] }), true);
});

test("the updater offers newer releases on its own channel only", async () => {
  const site = scratch("site"), tree = await packagedTree(BASE_FILES), tar = path.join(temp, `channel-${serial}.tar`);
  await buildPayload({ tree, platform: "linux-x64", out: tar, version: "0.1.0-preview.41" });
  publishFeed(site, { tar, platform: "linux-x64", version: "0.1.0-preview.41", release_id: "preview-41" });
  const { base } = await serve(site);
  const appPath = scratch("app");
  writeFileSync(path.join(appPath, "package.json"), JSON.stringify({ version: "0.1.0-preview.40", bfeUpdateChannel: "test-channel" }));
  const home = scratch("home");
  const make = (version, extra = {}) => createUpdater({ app: { isPackaged: true, getVersion: () => version, getAppPath: () => appPath }, fetch, env: { XDG_DATA_HOME: home }, platform: "linux", arch: "x64", keys: KEYS, base, ...extra });
  const updater = make("0.1.0-preview.40");
  assert.equal(updater.enabled, true);
  const found = await updater.check();
  assert.equal(found.release.version, "0.1.0-preview.41");
  assert.equal(await make("0.1.0-preview.41").check(), null, "already current");
  changeState(path.join(home, "battlefront-eternal"), (state) => { state.bad.push("0.1.0-preview.41"); });
  assert.equal(await updater.check(), null, "a release that failed here waits for a newer one");
  assert.equal(make("0.1.0-preview.40", { arch: "ia32" }).enabled, false, "no builds for this platform");
  assert.equal(make("0.1.0-preview.40", { keys: {} }).enabled, false, "no trust root, no updates");
  writeFileSync(path.join(appPath, "package.json"), JSON.stringify({ version: "0.1.0-preview.40" }));
  assert.equal(make("0.1.0-preview.40").enabled, false, "local builds have no channel");
  writeFileSync(path.join(appPath, "package.json"), JSON.stringify({ version: "0.1.0-preview.40", bfeUpdateChannel: "test-channel" }));
  await assert.rejects(make("0.1.0-preview.40", { base: "http://127.0.0.1:9/" }).check({ timeoutMs: 2000 }), "offline, the check fails and the caller carries on");
});

test("a build that cannot update leaves the installed client's updates alone", () => {
  const home = scratch("home"), root = path.join(home, "battlefront-eternal"), appPath = scratch("app");
  for (const version of ["0.1.0-preview.281", "0.1.0-preview.284"]) fakeInstall(root, version);
  writeFileSync(path.join(appPath, "package.json"), JSON.stringify({ version: "0.1.0" }));
  const make = (extra = {}) => createUpdater({ app: { isPackaged: true, getVersion: () => "0.1.0", getAppPath: () => appPath }, env: { XDG_DATA_HOME: home }, platform: "linux", arch: "x64", keys: KEYS, ...extra });
  assert.equal(make().enabled, false, "a local build has no channel");
  assert.deepEqual(make().settle(), []);
  assert.deepEqual(make({ app: { isPackaged: false, getVersion: () => "0.1.0", getAppPath: () => appPath } }).settle(), [], "unpackaged");
  writeFileSync(path.join(appPath, "package.json"), JSON.stringify({ version: "0.1.0", bfeUpdateChannel: "test-channel" }));
  assert.deepEqual(make({ env: { XDG_DATA_HOME: home, BFE_NO_UPDATE: "1" } }).settle(), [], "BFE_NO_UPDATE");
  assert.deepEqual(installedVersions(root).map((item) => item.version), ["0.1.0-preview.284", "0.1.0-preview.281"]);
});

test("the AppImage installer's launcher learns to start installed updates", async () => {
  const home = scratch("home"), file = path.join(home, ".local", "bin", "battlefront-eternal");
  assert.equal(await adoptInstallerLauncher({ home }), "missing");
  mkdirSync(path.dirname(file), { recursive: true });
  const original = '#!/usr/bin/env bash\nexport BFE_NO_DESKTOP_INTEGRATION=1\nexec /home/p/.local/share/battlefront-eternal/Battlefront-Eternal.AppImage --appimage-extract-and-run "$@"\n';
  writeFileSync(file, original, { mode: 0o755 });
  assert.equal(await adoptInstallerLauncher({ home }), "updated");
  const adopted = readFileSync(file, "utf8");
  assert.equal(adopted, updatingLauncher("/home/p/.local/share/battlefront-eternal/Battlefront-Eternal.AppImage"));
  assert.match(adopted, /current\/battlefront-eternal/);
  assert.match(adopted, /exec \/home\/p\/\.local\/share\/battlefront-eternal\/Battlefront-Eternal\.AppImage --appimage-extract-and-run "\$@"\n$/);
  assert.equal(await adoptInstallerLauncher({ home }), "current");
  writeFileSync(file, "#!/bin/sh\nexec something-else\n");
  assert.equal(await adoptInstallerLauncher({ home }), "foreign", "a launcher someone else wrote is left alone");
  assert.match(launcherScript("/opt/x.AppImage"), /current\/battlefront-eternal/);
});
