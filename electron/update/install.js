// Installs a release into <root>/versions/<version>: every file the
// manifest lists, taken from a build already on this computer when its
// hash matches (so a typical update downloads only the files that changed)
// and otherwise downloaded, decompressed and checked against the signed
// hash. It all happens in a staging folder that becomes the version folder
// in one rename once complete; a failure or cancel leaves nothing behind.
import { createHash, randomBytes } from "node:crypto";
import plain from "./fs.js";
import path from "node:path";
import { Readable, Transform } from "node:stream";
import { pipeline } from "node:stream/promises";
import { createZstdDecompress } from "node:zlib";
import { FEED } from "./feed.js";
import { INSTALL_RECORD, installedVersions, versionsDir } from "./state.js";
import { hashFile, readTreeIndex } from "./tree.js";

const fs = plain.promises;
const RETRIES = 3, IDLE_MS = 30000, MARGIN = 64 << 20;
const native = (root, relative) => path.join(root, ...relative.split("/"));
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export const blobUrl = (sha256, base = FEED) => new URL(`updates/blobs/${sha256.slice(0, 2)}/${sha256}.zst`, base).href;

// Windows refuses a rename while a virus scanner holds a new file; it lets
// go within moments.
async function rename(from, to) {
  for (let attempt = 1; ; attempt++) {
    try { return await fs.rename(from, to); }
    catch (error) {
      if (attempt >= 20 || !["EPERM", "EACCES", "EBUSY"].includes(error.code)) throw error;
      await sleep(100 * attempt);
    }
  }
}

// Files on this computer by hash, from each source build's own index.
// Builds without one are matched by path and size, then hashed.
async function localFiles(sources, tree) {
  const byHash = new Map();
  const offer = async (sha256, file, size) => {
    if (byHash.has(sha256)) return;
    try { if ((await fs.stat(file)).size === size) byHash.set(sha256, { file, size }); } catch {}
  };
  for (const dir of sources) {
    const index = await readTreeIndex(dir);
    if (index) {
      const wanted = new Set(tree.files.map((file) => file.sha256));
      for (const file of index) if (wanted.has(file.sha256)) await offer(file.sha256, native(dir, file.path), file.size);
      continue;
    }
    for (const file of tree.files) {
      if (byHash.has(file.sha256)) continue;
      const candidate = native(dir, file.path);
      try { if ((await fs.stat(candidate)).size === file.size && (await hashFile(candidate)) === file.sha256) byHash.set(file.sha256, { file: candidate, size: file.size }); } catch {}
    }
  }
  return byHash;
}

// Reuse is a hard link where the source is another of this user's versions
// on Linux (instant, no space), else a copy (reflinked where the filesystem
// can). Windows keeps copies: a linked file in use could never be deleted.
async function reuse(source, target, mode, { root, platform }) {
  if (platform !== "win32" && !path.relative(root, source).startsWith("..")) {
    try { if (((await fs.stat(source)).mode & 0o777) === mode) return await fs.link(source, target); } catch {}
  }
  await fs.copyFile(source, target, plain.constants.COPYFILE_FICLONE);
  if (platform !== "win32") await fs.chmod(target, mode);
}

// One file from the blob store: streamed through zstd, hashed and sized as
// it is written, renamed into place only if both match the manifest.
async function download(file, target, { fetch, base, signal, onBytes, platform }) {
  const partial = `${target}.part`;
  for (let attempt = 1; ; attempt++) {
    const idle = new AbortController();
    let timer = null, received = 0;
    const stalled = () => { clearTimeout(timer); timer = setTimeout(() => idle.abort(Error("The update download stalled")), IDLE_MS); };
    const both = signal ? AbortSignal.any([signal, idle.signal]) : idle.signal;
    try {
      stalled();
      const response = await fetch(blobUrl(file.sha256, base), { signal: both });
      if (!response.ok || !response.body) throw Error(`The update server answered ${response.status} for ${file.path}`);
      const hash = createHash("sha256");
      let size = 0;
      const count = new Transform({ transform(chunk, _encoding, done) { received += chunk.length; onBytes(chunk.length); stalled(); done(null, chunk); } });
      const check = new Transform({ transform(chunk, _encoding, done) { hash.update(chunk); size += chunk.length; done(null, chunk); } });
      await pipeline(Readable.fromWeb(response.body), count, createZstdDecompress(), check, plain.createWriteStream(partial), { signal: both });
      if (size !== file.size || hash.digest("hex") !== file.sha256) throw Error(`${file.path} did not match the signed release`);
      if (platform !== "win32") await fs.chmod(partial, file.mode);
      await rename(partial, target);
      return;
    } catch (error) {
      onBytes(-received);
      await fs.rm(partial, { force: true }).catch(() => {});
      if (signal?.aborted) throw signal.reason ?? error;
      if (attempt >= RETRIES) throw error;
      await sleep(1000 * attempt);
    } finally {
      clearTimeout(timer);
    }
  }
}

// `sources`: folders holding builds to reuse files from (the running one,
// the installed bootstrap, other installed versions). onProgress gets
// {phase, received, total, files, done} as it goes.
export async function installRelease({ root, release, tree, sources = [], fetch, base = FEED, signal, onProgress = () => {}, platform = process.platform, concurrency = 4 }) {
  const existing = installedVersions(root).find((item) => item.version === release.version);
  if (existing) return { ...existing, downloaded: 0, reused: tree.files.length };
  await fs.mkdir(versionsDir(root), { recursive: true });
  const local = await localFiles([...new Set(sources)].filter(Boolean), tree);
  const reused = [], fetched = [];
  for (const file of tree.files) (local.has(file.sha256) ? reused : fetched).push(file);
  const total = fetched.reduce((sum, file) => sum + file.zsize, 0);
  const needed = fetched.reduce((sum, file) => sum + file.size, 0) + (platform === "win32" ? reused.reduce((sum, file) => sum + file.size, 0) : 0) + MARGIN;
  const space = await fs.statfs(versionsDir(root)).catch(() => null);
  if (space && space.bavail * space.bsize < needed) throw Object.assign(Error(`The update needs ${Math.ceil(needed / 1e6)} MB free in ${root}.`), { code: "ENOSPC" });

  const staging = path.join(versionsDir(root), `.staging-${release.version}-${process.pid}-${randomBytes(4).toString("hex")}`);
  let received = 0, done = 0;
  const report = (phase) => onProgress({ phase, received, total, files: tree.files.length, done });
  try {
    const folders = new Set([...(tree.dirs ?? []), ...[...tree.files, ...(tree.links ?? [])].map((entry) => path.posix.dirname(entry.path)).filter((dir) => dir !== ".")]);
    await fs.mkdir(staging);
    for (const dir of [...folders].sort()) await fs.mkdir(native(staging, dir), { recursive: true });
    report("copying");
    // Several at once: thousands of small toolchain files copy far faster
    // in parallel, on SSDs especially.
    const copies = [...reused];
    const copier = async () => {
      for (let file = copies.shift(); file; file = copies.shift()) {
        signal?.throwIfAborted();
        await reuse(local.get(file.sha256).file, native(staging, file.path), file.mode, { root, platform });
        if (++done % 250 === 0) report("copying");
      }
    };
    await Promise.all(Array.from({ length: Math.min(8, copies.length) }, copier));
    report("downloading");
    // Largest first, so the slowest file never starts last.
    const queue = [...fetched].sort((a, b) => b.zsize - a.zsize);
    const worker = async () => {
      for (let file = queue.shift(); file; file = queue.shift()) {
        await download(file, native(staging, file.path), { fetch, base, signal, platform, onBytes: (bytes) => { received += bytes; report("downloading"); } });
        done++;
      }
    };
    await Promise.all(Array.from({ length: Math.min(concurrency, queue.length) }, worker));
    for (const link of tree.links ?? []) {
      if (platform === "win32") throw Error("This update holds links Windows cannot");
      await fs.symlink(link.target, native(staging, link.path));
    }
    report("finishing");
    const record = { version: release.version, release_id: release.release_id, revision: release.revision ?? null, platform: tree.platform, executable: tree.executable, files: tree.files.length, installedAt: new Date().toISOString() };
    await fs.writeFile(path.join(staging, INSTALL_RECORD), JSON.stringify(record, null, 2) + "\n");
    const target = path.join(versionsDir(root), release.version);
    try { await rename(staging, target); }
    catch (error) {
      // Another launch finished the same version first.
      if (!installedVersions(root).some((item) => item.version === release.version)) throw error;
      await fs.rm(staging, { recursive: true, force: true });
    }
    const installed = installedVersions(root).find((item) => item.version === release.version);
    if (!installed) throw Error("The update did not install completely");
    return { ...installed, downloaded: received, reused: reused.length };
  } catch (error) {
    await fs.rm(staging, { recursive: true, force: true, maxRetries: 3 }).catch(() => {});
    throw error;
  }
}
