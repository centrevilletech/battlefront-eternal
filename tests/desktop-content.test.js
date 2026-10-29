import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { after, test } from "node:test";
import { createHandler, CSP, locate, parseRange, resolveBelow } from "../electron/content.js";

const temp = mkdtempSync(path.join(tmpdir(), "bfe-content-"));
const appRoot = path.join(temp, "dist");
const contentRoot = path.join(temp, "game-data");
mkdirSync(path.join(appRoot, "assets"), { recursive: true });
mkdirSync(path.join(contentRoot, "audio", "nab"), { recursive: true });
writeFileSync(path.join(appRoot, "index.html"), "<!doctype html>");
writeFileSync(path.join(appRoot, "assets", "main.js"), "export {}");
writeFileSync(path.join(contentRoot, "audio", "nab", "bank.bin"), Buffer.from([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]));
writeFileSync(path.join(temp, "secret.txt"), "outside");
after(() => rmSync(temp, { recursive: true, force: true }));

const handler = createHandler(() => ({ appRoot, contentRoot }));
const get = (url, headers = {}) => handler(new Request(url, { headers }));

test("engine files come from the build and game data from the content folder", () => {
  const roots = { appRoot, contentRoot };
  assert.equal(locate("/", roots), path.join(appRoot, "index.html"));
  assert.equal(locate("/assets/main.js", roots), path.join(appRoot, "assets", "main.js"));
  assert.equal(locate("/audio/nab/bank.bin", roots), path.join(contentRoot, "audio", "nab", "bank.bin"));
  assert.equal(locate("/maps/nab2/world.json", roots), path.join(contentRoot, "maps", "nab2", "world.json"));
  assert.equal(locate("/maps/nab2/world.json", { appRoot, contentRoot: null }), null);
  assert.equal(locate("/__desktop/prepare.html", { ...roots, uiRoot: "/ui" }), path.resolve("/ui/prepare.html"));
  assert.equal(locate("/__desktop/../secret", { ...roots, uiRoot: "/ui" }), path.resolve("/ui/secret"));
  assert.equal(locate("/__desktop/prepare.html", roots), null);
});

test("paths cannot escape their root", () => {
  assert.equal(resolveBelow(appRoot, "/../secret.txt"), path.join(appRoot, "secret.txt"));
  assert.equal(resolveBelow(appRoot, "/%2e%2e/secret.txt"), path.join(appRoot, "secret.txt"));
  assert.equal(resolveBelow(appRoot, "/%2e%2e%2f%2e%2e%2fsecret.txt"), path.join(appRoot, "secret.txt"));
  assert.equal(resolveBelow(appRoot, "/a%00b"), null);
  assert.equal(resolveBelow(appRoot, "/%E0%A4%A"), null);
});

test("range headers", () => {
  assert.deepEqual(parseRange("bytes=2-5", 10), { start: 2, end: 5 });
  assert.deepEqual(parseRange("bytes=7-", 10), { start: 7, end: 9 });
  assert.deepEqual(parseRange("bytes=-3", 10), { start: 7, end: 9 });
  assert.deepEqual(parseRange("bytes=4-99", 10), { start: 4, end: 9 });
  assert.equal(parseRange("bytes=10-12", 10), "unsatisfiable");
  assert.equal(parseRange("bytes=5-2", 10), "unsatisfiable");
  assert.equal(parseRange("items=1-2", 10), null);
  assert.equal(parseRange("bytes=-", 10), null);
});

test("serves files with types, CSP and byte ranges", async () => {
  const page = await get("app://bfe/");
  assert.equal(page.status, 200);
  assert.match(page.headers.get("content-type"), /text\/html/);
  assert.equal(page.headers.get("content-security-policy"), CSP);
  assert.equal(await page.text(), "<!doctype html>");

  const script = await get("app://bfe/assets/main.js");
  assert.match(script.headers.get("content-type"), /text\/javascript/);

  const whole = await get("app://bfe/audio/nab/bank.bin");
  assert.equal(whole.headers.get("content-length"), "10");
  assert.equal((await whole.arrayBuffer()).byteLength, 10);

  const part = await get("app://bfe/audio/nab/bank.bin", { Range: "bytes=2-5" });
  assert.equal(part.status, 206);
  assert.equal(part.headers.get("content-range"), "bytes 2-5/10");
  assert.deepEqual([...new Uint8Array(await part.arrayBuffer())], [2, 3, 4, 5]);

  assert.equal((await get("app://bfe/audio/nab/bank.bin", { Range: "bytes=20-" })).status, 416);
});

test("missing, escaping and foreign-host requests are 404", async () => {
  assert.equal((await get("app://bfe/maps/none/world.json")).status, 404);
  assert.equal((await get("app://bfe/assets")).status, 404);
  assert.equal((await get("app://bfe/%2e%2e/secret.txt")).status, 404);
  assert.equal((await get("app://other/index.html")).status, 404);
});
