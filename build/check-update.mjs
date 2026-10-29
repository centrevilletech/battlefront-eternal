// Release check of the packaged updater on this runner's own platform
// (Windows included, which no developer machine here runs): the packaged
// runtime, in Node mode, installs this build from its own update payload
// the way a client updates, downloading the payload's new files from a
// local server and reusing the rest from a copy of the build. Every file
// is checked, then the installed copy is run.
//
//   node build/check-update.mjs <app folder> <platform> <payload.tar>
import { spawn, spawnSync } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import os from "node:os";
import path from "node:path";
import { TREE_INDEX } from "../electron/update/tree.js";

const [app, platform, payload] = process.argv.slice(2);
if (!app || !platform || !payload) throw Error("usage: node build/check-update.mjs <app folder> <platform> <payload.tar>");
const executable = path.resolve(app, platform.startsWith("win32") ? "Battlefront Eternal.exe" : "battlefront-eternal");

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

// Runs while this process keeps serving its downloads.
function run(file, args, env) {
  return new Promise((resolve) => {
    const child = spawn(file, args, { env });
    let stdout = "", stderr = "";
    child.stdout.on("data", (chunk) => (stdout += chunk));
    child.stderr.on("data", (chunk) => (stderr += chunk));
    child.on("close", (status) => resolve({ status, stdout, stderr }));
  });
}

const CHECK = `
  import { readFile } from "node:fs/promises";
  import path from "node:path";
  import { pathToFileURL } from "node:url";
  const [archive, root, source, base, treeFile, platform] = process.argv.slice(2);
  const { installRelease } = await import(pathToFileURL(path.join(archive, "electron/update/install.js")));
  const { hashFile, validateTree } = await import(pathToFileURL(path.join(archive, "electron/update/tree.js")));
  const tree = validateTree(JSON.parse(await readFile(treeFile, "utf8")), { platform });
  const installed = await installRelease({ root, release: { version: tree.version, release_id: "check" }, tree, sources: [source], fetch, base, platform: process.platform });
  for (const file of tree.files) if ((await hashFile(path.join(installed.dir, ...file.path.split("/")))) !== file.sha256) throw Error("installed " + file.path + " does not match");
  console.log(JSON.stringify({ executable: installed.executable, reused: installed.reused, downloaded: installed.downloaded, files: tree.files.length }));
`;

const work = mkdtempSync(path.join(os.tmpdir(), "bfe-update-check-"));
try {
  const entries = readTar(payload), tree = JSON.parse(entries.get(`update-${platform}.json`));
  const fresh = new Set([...entries.keys()].map((name) => /^blobs\/([a-f0-9]{64})\.zst$/.exec(name)?.[1]).filter(Boolean));
  if (!fresh.size) throw Error("The payload holds no new file to download");
  // Files whose blob is in the payload must download; the rest come from a
  // copy of the build without them, as from the version a client runs.
  const source = path.join(work, "source");
  cpSync(path.resolve(app), source, { recursive: true, verbatimSymlinks: true });
  const kept = [];
  for (const file of tree.files) {
    if (fresh.has(file.sha256)) rmSync(path.join(source, ...file.path.split("/")), { force: true });
    else kept.push({ path: file.path, size: file.size, sha256: file.sha256 });
  }
  writeFileSync(path.join(source, ...TREE_INDEX.split("/")), JSON.stringify({ schema: 1, files: kept }));
  const server = createServer((request, response) => {
    const digest = /^\/updates\/blobs\/[a-f0-9]{2}\/([a-f0-9]{64})\.zst$/.exec(request.url)?.[1];
    const blob = digest && entries.get(`blobs/${digest}.zst`);
    if (!blob) response.writeHead(404).end();
    else response.writeHead(200, { "content-length": blob.length }).end(blob);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const script = path.join(work, "check.mjs"), treeFile = path.join(work, "tree.json"), root = path.join(work, "root");
  writeFileSync(script, CHECK);
  writeFileSync(treeFile, JSON.stringify(tree));
  mkdirSync(root);
  const env = { ...process.env, ELECTRON_RUN_AS_NODE: "1" };
  const result = await run(executable, [script, path.resolve(app, "resources", "app.asar"), root, source, `http://127.0.0.1:${server.address().port}/`, treeFile, platform], env);
  server.close();
  if (result.status !== 0) throw Error(`The packaged updater failed:\n${result.stderr}${result.stdout}`);
  const report = JSON.parse(result.stdout.trim().split("\n").pop());
  if (!(report.downloaded > 0) || report.reused !== tree.files.filter((file) => !fresh.has(file.sha256)).length) throw Error(`Unexpected install: ${result.stdout}`);
  const version = spawnSync(report.executable, ["-e", "console.log(process.versions.electron)"], { env, encoding: "utf8" });
  if (version.status !== 0 || !/^\d+\.\d+\.\d+/.test(version.stdout.trim())) throw Error(`The installed copy does not run: ${version.stderr}`);
  console.log(`Packaged updater installed ${report.files} files on ${platform}: ${report.reused} reused, ${(report.downloaded / 1e6).toFixed(1)} MB downloaded; the copy runs Electron ${version.stdout.trim()}`);
} finally {
  rmSync(work, { recursive: true, force: true, maxRetries: 3 });
}
