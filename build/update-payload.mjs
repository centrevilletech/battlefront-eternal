// Turns a packaged app folder (electron-builder's linux-unpacked,
// linux-arm64-unpacked or win-unpacked) into an update payload for the
// desktop updater (docs/desktop-updates.md): one tar holding the platform's
// file manifest (update-<platform>.json: every file's path, size, SHA-256,
// compressed size and mode, links, empty folders) and, zstd-compressed and
// named by the SHA-256 of their contents, the files the download server
// does not hold yet (--known: its blobs.json). build/publish-distribution.py
// adds the blobs to the server's store and signs the release.
//
//   node build/update-payload.mjs <app folder> <platform> <out.tar> [--known <url or file>]
//
// Linux payloads leave out chrome-sandbox: in the user's folder it can never
// be the setuid helper Chromium would then insist on (electron/update/launch.js).
import { createHash } from "node:crypto";
import { once } from "node:events";
import { createReadStream, createWriteStream, promises as fs } from "node:fs";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { promisify } from "node:util";
import zlib from "node:zlib";
import { TREE_INDEX, readTreeIndex, validateTree, walkTree } from "../electron/update/tree.js";

// Compression runs on libuv's pool, created at its first use (after this).
process.env.UV_THREADPOOL_SIZE ??= String(Math.max(4, Math.min(16, os.availableParallelism())));

const compress = promisify(zlib.zstdCompress), decompress = promisify(zlib.zstdDecompress);
const PLATFORMS = { "linux-x64": "battlefront-eternal", "linux-arm64": "battlefront-eternal", "win32-x64": "Battlefront Eternal.exe" };

// zstd 19, told each file's size so small files get small tables (without
// it every one allocated the full ones: minutes of system time per tree).
// Files over 32 MB (the Electron binary, ffmpeg) take level 12: seconds
// instead of minutes for about a tenth more bytes. zstd's own threads are
// no option: Node 22's asynchronous zstd writes empty frames with them and
// Electron's Node refuses them. Done once per file content: later builds
// find it in the server's store.
const zstdOptions = (size) => ({ pledgedSrcSize: size, params: {
  [zlib.constants.ZSTD_c_compressionLevel]: size > 32 << 20 ? 12 : 19,
  [zlib.constants.ZSTD_c_checksumFlag]: 1,
} });

async function knownBlobs(source) {
  if (!source) return {};
  try {
    const text = /^https?:/.test(source)
      ? await fetch(source, { cache: "no-store" }).then((response) => (response.ok ? response.text() : "{}"))
      : await fs.readFile(source, "utf8");
    const blobs = JSON.parse(text).blobs;
    return blobs && typeof blobs === "object" ? blobs : {};
  } catch (error) {
    console.warn(`No known blobs (${error.message}); the payload carries every file.`);
    return {};
  }
}

function tarHeader(name, size) {
  const header = Buffer.alloc(512, 0);
  header.write(name, 0, 100, "utf8");
  header.write("0000644\0", 100);
  header.write("0000000\0", 108);
  header.write("0000000\0", 116);
  header.write(size.toString(8).padStart(11, "0") + "\0", 124);
  header.write("00000000000\0", 136);
  header.write("        ", 148);
  header.write("0", 156);
  header.write("ustar\0" + "00", 257);
  let sum = 0;
  for (const byte of header) sum += byte;
  header.write(sum.toString(8).padStart(6, "0") + "\0 ", 148);
  return header;
}

async function writeTar(out, entries) {
  const stream = createWriteStream(out);
  const write = async (chunk) => { if (!stream.write(chunk)) await once(stream, "drain"); };
  for (const { name, file, data } of entries) {
    const size = data ? data.length : (await fs.stat(file)).size;
    await write(tarHeader(name, size));
    if (data) await write(data);
    else for await (const chunk of createReadStream(file)) await write(chunk);
    if (size % 512) await write(Buffer.alloc(512 - (size % 512)));
  }
  await write(Buffer.alloc(1024));
  stream.end();
  await once(stream, "close");
}

export async function buildPayload({ tree: root, platform, out, known = {}, version }) {
  const executable = PLATFORMS[platform];
  if (!executable) throw Error(`Unknown platform ${platform}`);
  const exclude = (name) => name === (platform.startsWith("linux") ? "chrome-sandbox" : null);
  const { files, links, dirs } = await walkTree(root, { exclude });
  // The packaged index must not describe any file otherwise than it is, or
  // installed builds would offer stale ones for reuse. Files a target adds
  // after packing (the NSIS target's resources/elevate.exe) are simply not
  // in it: clients download those instead of reusing them.
  const index = await readTreeIndex(root);
  if (!index) throw Error(`${root} has no ${TREE_INDEX} (build/tree-index.cjs)`);
  const indexed = new Map(index.map((file) => [file.path, file])), unindexed = [];
  for (const file of files) {
    if (file.path === TREE_INDEX) continue;
    const entry = indexed.get(file.path);
    if (!entry) unindexed.push(file.path);
    else if (entry.sha256 !== file.sha256 || entry.size !== file.size) throw Error(`${TREE_INDEX} does not match ${file.path}`);
  }
  if (unindexed.length > 16) throw Error(`${TREE_INDEX} lacks ${unindexed.length} files, ${unindexed.slice(0, 3).join(", ")} among them`);
  if (unindexed.length) console.warn(`Not in ${TREE_INDEX} (added after packing): ${unindexed.join(", ")}`);
  let content = null;
  try { content = JSON.parse(await fs.readFile(path.join(root, "resources", "tools", "content-version.json"), "utf8")); } catch {}

  const work = await fs.mkdtemp(path.join(os.tmpdir(), "bfe-payload-"));
  try {
    const sizes = new Map(), fresh = new Map();
    for (const file of files) if (Number.isSafeInteger(known[file.sha256])) sizes.set(file.sha256, known[file.sha256]);
    const queue = [...new Map(files.filter((file) => !sizes.has(file.sha256)).map((file) => [file.sha256, file])).values()].sort((a, b) => b.size - a.size);
    const worker = async () => {
      for (let file = queue.shift(); file; file = queue.shift()) {
        const data = await fs.readFile(path.join(root, ...file.path.split("/")));
        const packed = await compress(data, zstdOptions(data.length));
        const back = await decompress(packed, { maxOutputLength: data.length + 1 });
        if (createHash("sha256").update(back).digest("hex") !== file.sha256) throw Error(`zstd round trip failed for ${file.path}`);
        const blob = path.join(work, `${file.sha256}.zst`);
        await fs.writeFile(blob, packed);
        sizes.set(file.sha256, packed.length);
        fresh.set(file.sha256, blob);
      }
    };
    await Promise.all(Array.from({ length: Math.max(1, Number(process.env.UV_THREADPOOL_SIZE) || 4) }, worker));

    const manifest = validateTree({
      schema: 1, platform, version, executable, content,
      files: files.map(({ path: name, size, sha256, mode }) => ({ path: name, size, sha256, zsize: sizes.get(sha256), mode })),
      links, dirs,
    }, { platform, version });
    const json = Buffer.from(JSON.stringify(manifest) + "\n");
    await writeTar(out, [{ name: `update-${platform}.json`, data: json }, ...[...fresh].sort().map(([sha256, file]) => ({ name: `blobs/${sha256}.zst`, file }))]);
    const bytes = files.reduce((sum, file) => sum + file.size, 0), packed = [...new Set(files.map((file) => file.sha256))].reduce((sum, sha) => sum + sizes.get(sha), 0);
    return { files: files.length, links: links.length, bytes, packed, fresh: fresh.size, freshBytes: [...fresh.keys()].reduce((sum, sha) => sum + sizes.get(sha), 0) };
  } finally {
    await fs.rm(work, { recursive: true, force: true });
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const args = process.argv.slice(2), flag = args.indexOf("--known");
  const known = flag >= 0 ? args.splice(flag, 2)[1] : null;
  const [tree, platform, out] = args;
  if (!tree || !platform || !out) {
    console.error("usage: node build/update-payload.mjs <app folder> <platform> <out.tar> [--known <url or file>]");
    process.exit(2);
  }
  const { version } = JSON.parse(await fs.readFile("package.json", "utf8"));
  const result = await buildPayload({ tree: path.resolve(tree), platform, out: path.resolve(out), known: await knownBlobs(known), version });
  console.log(`${platform} ${version}: ${result.files} files, ${result.links} links, ${(result.bytes / 1e6).toFixed(0)} MB (${(result.packed / 1e6).toFixed(0)} MB packed); ${result.fresh} new blobs, ${(result.freshBytes / 1e6).toFixed(1)} MB, in ${out}`);
}
