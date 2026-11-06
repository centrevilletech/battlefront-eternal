#!/usr/bin/env node
// Archiver for the C/C++ parts of the WebAssembly builds (AR_wasm32_unknown_unknown,
// set by tools/build-rust.mjs). It runs the pinned llvm-ar, then rewrites the
// archive with its members in a machine-independent order.
//
// Why: the Basis transcoder's libc/libc++ build scripts list their sources with
// an unsorted directory read, so each archive's member order follows the file
// system that unpacked the crate. wasm-ld lays out functions in the order it
// pulls members, so the same code linked on another machine gave different
// .wasm bytes. Sorting here makes the order a property of the sources alone.
// Members are ordered by object name without cc's path-hash prefix, then by a
// hash of their bytes, which also separates same-named objects from different
// directories. Names and contents are kept; only the order changes.
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname, basename } from 'node:path';

const real = process.env.BFE_ARCHIVER;
if (!real) { console.error('ar-sorted: BFE_ARCHIVER names the real archiver'); process.exit(2); }
const args = process.argv.slice(2);
const ran = spawnSync(real, args, { stdio: 'inherit' });
if (ran.status !== 0) process.exit(ran.status ?? 1);
// The archive is the first argument after the operation letters that names an
// existing .a file. Listing or extraction calls leave it unchanged.
const operation = args.find((arg) => !arg.startsWith('-')) ?? '';
const archive = args.slice(args.indexOf(operation) + 1).find((arg) => arg.endsWith('.a'));
if (!archive || !existsSync(archive) || !/[qrsd]/.test(operation)) process.exit(0);

// GNU archive members, with long names resolved from the "//" table. The
// symbol table ("/" or "/SYM64/") is dropped; llvm-ar rebuilds it below.
function members(bytes) {
  if (bytes.toString('latin1', 0, 8) !== '!<arch>\n') throw Error(`ar-sorted: ${archive} is not an ar archive`);
  const out = [];
  let names = null;
  for (let at = 8; at < bytes.length;) {
    const header = bytes.toString('latin1', at, at + 60);
    if (header.slice(58, 60) !== '`\n') throw Error(`ar-sorted: malformed member header in ${archive}`);
    const raw = header.slice(0, 16).trimEnd(), size = Number(header.slice(48, 58).trim());
    const data = bytes.subarray(at + 60, at + 60 + size);
    at += 60 + size + (size & 1);
    if (raw === '/' || raw === '/SYM64/') continue;
    if (raw === '//') { names = data.toString('latin1'); continue; }
    let name;
    if (/^\/\d+$/.test(raw)) {
      if (names === null) throw Error(`ar-sorted: long name without a name table in ${archive}`);
      const offset = Number(raw.slice(1));
      name = names.slice(offset, names.indexOf('\n', offset)).replace(/\/$/, '');
    } else name = raw.replace(/\/$/, '');
    out.push({ name, data });
  }
  return out;
}

const list = members(readFileSync(archive)).map((m) => ({ ...m,
  key: m.name.replace(/^[0-9a-f]{16}-/, ''), hash: createHash('sha256').update(m.data).digest('hex') }));
list.sort((a, b) => (a.key < b.key ? -1 : a.key > b.key ? 1 : a.hash < b.hash ? -1 : a.hash > b.hash ? 1 : 0));
// Each member in its own folder keeps its name even when names repeat.
const work = mkdtempSync(join(tmpdir(), 'ar-sorted-'));
try {
  const files = list.map((m, i) => { const dir = join(work, String(i)); mkdirSync(dir); const file = join(dir, m.name); writeFileSync(file, m.data); return file; });
  const sorted = join(dirname(archive), `.${basename(archive)}.sorted`);
  const rebuilt = spawnSync(real, ['rcsD', '--format=gnu', sorted, ...files], { stdio: 'inherit' });
  if (rebuilt.status !== 0) process.exit(rebuilt.status ?? 1);
  renameSync(sorted, archive);
} finally { rmSync(work, { recursive: true, force: true }); }
