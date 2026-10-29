// An installed app as a list of files: the format the release build writes
// (build/update-payload.mjs, build/after-sign.cjs) and the updater checks
// before trusting a byte of it. Paths are relative, `/`-separated and never
// leave the version's folder; links (Linux) point inside it too.
import { createHash } from "node:crypto";
import plain from "./fs.js";
import path from "node:path";

const fs = plain.promises;

// Every packaged build carries the hashes of its own files, so an update can
// reuse what is already on disk without reading it all again.
export const TREE_INDEX = "resources/bfe-tree.json";
const MAX_FILES = 100000, MAX_TOTAL = 8 * 1024 ** 3;
const HEX = /^[a-f0-9]{64}$/;

export function safeRelative(value) {
  if (typeof value !== "string" || !value || value.length > 512) return false;
  return value.split("/").every((part) => part && part !== "." && part !== ".." && !/[\\:*?"<>|\u0000-\u001f]/.test(part));
}

// Where a link lands, relative to the version folder, or null if outside.
export function linkTarget(linkPath, target) {
  if (typeof target !== "string" || !target || target.startsWith("/") || target.includes("\\")) return null;
  const resolved = path.posix.normalize(path.posix.join(path.posix.dirname(linkPath), target));
  return safeRelative(resolved) ? resolved : null;
}

export function validateTree(tree, { platform, version } = {}) {
  if (tree?.schema !== 1 || (platform && tree.platform !== platform) || (version && tree.version !== version)) throw Error("Update manifest is for another build");
  if (!safeRelative(tree.executable) || !Array.isArray(tree.files) || !Array.isArray(tree.links ?? []) || !Array.isArray(tree.dirs ?? [])) throw Error("Update manifest is malformed");
  if (tree.files.length > MAX_FILES) throw Error("Update manifest is too large");
  // Windows folders ignore case; Linux ones hold names that differ only by
  // it (the toolchain's terminfo has both E/Eterm and e/eterm).
  const seen = new Set(), folded = String(tree.platform).startsWith("win32");
  const claim = (name) => {
    const key = folded ? name.toLowerCase() : name;
    if (seen.has(key)) throw Error(`Update manifest lists ${name} twice`);
    seen.add(key);
  };
  let total = 0;
  for (const file of tree.files) {
    if (!safeRelative(file?.path) || !HEX.test(file.sha256) || !Number.isSafeInteger(file.size) || file.size < 0 || !Number.isSafeInteger(file.zsize) || file.zsize < 0 || ![0o644, 0o755].includes(file.mode)) throw Error("Update manifest has an invalid file entry");
    claim(file.path);
    total += file.size;
  }
  if (total > MAX_TOTAL) throw Error("Update manifest is too large");
  for (const link of tree.links ?? []) {
    if (!safeRelative(link?.path) || !linkTarget(link.path, link.target)) throw Error("Update manifest has an invalid link");
    claim(link.path);
  }
  for (const dir of tree.dirs ?? []) if (!safeRelative(dir)) throw Error("Update manifest has an invalid folder");
  if (!tree.files.some((file) => file.path === tree.executable)) throw Error("Update manifest has no executable");
  return tree;
}

export function hashFile(file) {
  return new Promise((resolve, reject) => {
    const hash = createHash("sha256");
    plain.createReadStream(file).on("error", reject).on("data", (chunk) => hash.update(chunk)).on("end", () => resolve(hash.digest("hex")));
  });
}

// Every file (size, sha256, executable or not), link and empty folder under
// `root`. `exclude(relative)` drops entries (and whole folders).
export async function walkTree(root, { exclude = () => false } = {}) {
  const files = [], links = [], dirs = [];
  async function visit(relative) {
    const entries = await fs.readdir(path.join(root, ...relative.split("/").filter(Boolean)), { withFileTypes: true });
    let kept = 0;
    for (const entry of entries.sort((a, b) => (a.name < b.name ? -1 : 1))) {
      const name = relative ? `${relative}/${entry.name}` : entry.name;
      if (exclude(name)) continue;
      const full = path.join(root, ...name.split("/"));
      kept++;
      if (entry.isSymbolicLink()) links.push({ path: name, target: (await fs.readlink(full)).split(path.sep).join("/") });
      else if (entry.isDirectory()) await visit(name);
      else if (entry.isFile()) {
        const stat = await fs.stat(full);
        files.push({ path: name, size: stat.size, mode: stat.mode & 0o111 ? 0o755 : 0o644, sha256: await hashFile(full) });
      }
    }
    if (!kept && relative) dirs.push(relative);
  }
  await visit("");
  return { files, links, dirs };
}

// A build's own file list (TREE_INDEX), or null when it has none.
export async function readTreeIndex(dir) {
  try {
    const index = JSON.parse(await fs.readFile(path.join(dir, ...TREE_INDEX.split("/")), "utf8"));
    if (index?.schema !== 1 || !Array.isArray(index.files)) return null;
    return index.files.filter((file) => safeRelative(file?.path) && HEX.test(file.sha256) && Number.isSafeInteger(file.size));
  } catch {
    return null;
  }
}
