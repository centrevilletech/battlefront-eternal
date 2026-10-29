#!/usr/bin/env node
// Diagnose gameplay compatibility using exactly the runtime fingerprint.
//
//   node tools/content-report.mjs <prepared-folder> [query]     one battle's hash and files
//   node tools/content-report.mjs <prepared-folder> --battles   every regular battle's, as JSON
//   node tools/content-report.mjs --compare <a> <b>             the battles that differ, and the files
//
// <a> and <b> are prepared folders, or --battles output saved as .json (a
// server's, taken there with its own engine). --compare exits 1 when any
// battle differs. A game server's data must equal what a fresh preparation
// by the same engine makes, on any platform, or every player who prepares
// then can only spectate (docs/desktop.md, "Prepared content compatibility").
import { realpathSync } from 'node:fs';
import { readFile, readdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { contentHash, contentManifest } from '../src/runtime/content-hash.js';
import { battlesOf } from '../src/mission.js';

const reader = (root) => async (name) => {
  try { return new Response(await readFile(path.join(root, name))); }
  catch (error) { if (error.code === 'ENOENT') return new Response('', { status: 404 }); throw error; }
};

async function report(root, query) {
  const fetcher = reader(root);
  return { hash: await contentHash(query, fetcher), files: Object.fromEntries(await contentManifest(query, fetcher)) };
}

// Every regular battle in a prepared folder, as a server's rotation asks
// for it: a map's Clone Wars battle as side=rep, its Galactic Civil War one
// as side=all.
export async function battles(root) {
  const out = {};
  for (const map of (await readdir(path.join(root, 'maps'))).sort()) {
    let world;
    try { world = JSON.parse(await readFile(path.join(root, 'maps', map, 'map.json'), 'utf8')); } catch { continue; }
    const eras = new Set(battlesOf(world).filter((battle) => battle.playable && !battle.historical).map((battle) => battle.era));
    for (const [era, side] of [['cw', 'rep'], ['gcw', 'all']]) {
      if (!eras.has(era)) continue;
      const query = `?map=${map}&side=${side}`;
      try { out[query] = await report(root, query); } catch (error) { out[query] = { error: error.message }; }
    }
  }
  return out;
}

// [{ query, files }] for each battle the two hold differently (files: the
// fingerprinted paths whose digests differ; null when one lacks the battle
// or could not fingerprint it).
export function compare(a, b) {
  const out = [];
  for (const query of [...new Set([...Object.keys(a), ...Object.keys(b)])].sort()) {
    const x = a[query], y = b[query];
    if (!x?.hash || !y?.hash) { out.push({ query, files: null, missing: !x?.hash ? 'first' : 'second' }); continue; }
    if (x.hash === y.hash) continue;
    out.push({ query, files: [...new Set([...Object.keys(x.files), ...Object.keys(y.files)])].sort().filter((file) => x.files[file] !== y.files[file]) });
  }
  return out;
}

const load = async (source) => source.endsWith('.json') ? JSON.parse(await readFile(source, 'utf8')) : battles(source);

async function main(args) {
  if (args[0] === '--compare' && args.length === 3) {
    const [a, b] = await Promise.all([load(args[1]), load(args[2])]);
    const differ = compare(a, b);
    for (const { query, files, missing } of differ) console.log(`${query}: ${files ? files.join(' ') : `not fingerprinted in the ${missing}`}`);
    const total = new Set([...Object.keys(a), ...Object.keys(b)]).size;
    console.log(differ.length ? `${differ.length} of ${total} battles differ` : `all ${total} battles match`);
    return differ.length ? 1 : 0;
  }
  const [root, query = '?map=nab2&side=rep'] = args;
  if (!root || root.startsWith('--')) throw Error('Usage: node tools/content-report.mjs <prepared-folder> [query | --battles] | --compare <a> <b>');
  console.log(JSON.stringify(query === '--battles' ? await battles(root) : { query, ...(await report(root, query)) }, null, 2));
  return 0;
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) process.exitCode = await main(process.argv.slice(2));
