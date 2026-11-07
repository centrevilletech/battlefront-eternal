// Checks the native cold setup against the whole-world input corpus
// (docs/world-replay.md): for every recorded battle, the query, the water
// setup row, the fetched files (URLs and SHA-256) and every tick row the
// host stamped, against what the setup-rows example computes from the
// player's content. Node only decodes the recording and compares; every
// number comes from the Rust side.
//
//   node tools/native-setup-compare.mjs --corpus DIR --content DIR --driver BIN [--only REGEX] [--out FILE]
//
// The driver is rust/target/<profile>/examples/setup-rows (cargo build -p
// bfe-content-native --example setup-rows). Exit 1 when any case differs.
import fs from 'node:fs';
import path from 'node:path';
import { gunzipSync } from 'node:zlib';
import { spawn, spawnSync } from 'node:child_process';
const argv = process.argv.slice(2);
const opt = (name, fallback = null) => { const at = argv.indexOf(`--${name}`); return at < 0 ? fallback : argv[at + 1]; };
const corpus = opt('corpus'), content = opt('content', process.env.BFE_CONTENT_DIR), driver = opt('driver'), only = opt('only'), out = opt('out');
if (!corpus || !content || !driver) { console.error('usage: node tools/native-setup-compare.mjs --corpus DIR --content DIR --driver BIN [--only REGEX] [--out FILE]'); process.exit(2); }
const cases = fs.readdirSync(corpus).filter(name => fs.existsSync(path.join(corpus, name, 'setup.json'))).filter(name => !only || new RegExp(only).test(name)).sort();
if (!cases.length) throw Error('no recorded battle matches');
// world-hash.mjs's own argument reading, for the options the driver takes.
const battleArgs = recorded => {
  const take = (name, fallback) => { const i = recorded.indexOf(`--${name}`); return i < 0 ? fallback : recorded[i + 1]; };
  const args = ['--map', take('map', 'nab2'), '--side', take('side', 'rep'), '--seed', take('seed', '1'), '--ai', take('ai', 'classic')];
  if (recorded.includes('--heroes')) args.push('--heroes');
  if (take('unit', null) !== null) args.push('--unit', take('unit'));
  return args;
};
// Tick rows cross as hex f64 bits: a signed zero or NaN payload survives.
const rowWords = encoded => { const bytes = Buffer.from(encoded, 'base64'), words = []; for (let at = 0; at < bytes.length; at += 8) words.push(bytes.readBigUInt64LE(at).toString(16).padStart(16, '0')); return words; };
function checkRows(args, frames) {
  return new Promise((resolve, reject) => {
    const child = spawn(driver, [content, ...args, '--check-rows'], { stdio: ['pipe', 'pipe', 'inherit'] });
    let text = '';
    child.stdout.on('data', chunk => { text += chunk; });
    child.on('error', reject);
    child.on('close', code => code === 0 ? resolve(text.trim().split('\n').pop()) : reject(Error(`setup-rows exited ${code}`)));
    const lines = [];
    for (const frame of frames) for (const encoded of frame.world) lines.push(JSON.stringify({ battle: frame.battle, world: rowWords(encoded) }));
    child.stdin.end(lines.join('\n') + '\n');
  });
}
const results = [];
let failures = 0;
for (const label of cases) {
  const dir = path.join(corpus, label), setup = JSON.parse(fs.readFileSync(path.join(dir, 'setup.json'), 'utf8'));
  const args = battleArgs(setup.argv), problems = [];
  const run = spawnSync(driver, [content, ...args], { encoding: 'utf8', maxBuffer: 1 << 26 });
  if (run.status !== 0) { problems.push(`driver: ${run.stderr.trim()}`); }
  const native = run.status === 0 ? JSON.parse(run.stdout.trim().split('\n')[0]) : null;
  let rows = null;
  if (native) {
    if (native.query !== setup.options.query) problems.push(`query ${native.query} vs ${setup.options.query}`);
    if (native.era !== setup.options.era) problems.push(`era ${native.era} vs ${setup.options.era}`);
    for (const battle of setup.battles) if (battle.worldSetup !== native.worldSetup) problems.push(`setup row ${native.worldSetup} vs ${battle.worldSetup} (battle ${battle.index})`);
    const recorded = setup.assets, urls = new Set(native.assets.map(a => a.url));
    for (const url of Object.keys(recorded)) if (!urls.has(url)) problems.push(`not fetched natively: ${url}`);
    // The page fetches a soldier's body when its class first spawns; the
    // native loader prepares every soldier and droideka class before the
    // battle (units.js prepareSoldiers), so the recording's set is a subset.
    // Bodies prepared ahead are counted; any other extra file is a difference.
    let ahead = 0;
    for (const asset of native.assets) {
      if (!Object.hasOwn(recorded, asset.url)) { if (/^\/sides\/[^/]+\/actors\/[^/]+(\.json|\.bin|_index\.bin)$/.test(asset.url)) ahead++; else problems.push(`fetched only natively: ${asset.url}`); continue; }
      const want = recorded[asset.url];
      if ((want?.sha256 ?? null) !== asset.sha256 || (want?.bytes ?? 0) !== asset.bytes) problems.push(`bytes differ: ${asset.url}`);
    }
    const order = native.assets.map(a => a.url).filter(u => Object.hasOwn(recorded, u)), recordedOrder = Object.keys(recorded).filter(u => urls.has(u));
    const sameOrder = order.length === recordedOrder.length && order.every((u, i) => u === recordedOrder[i]);
    const frames = JSON.parse(gunzipSync(fs.readFileSync(path.join(dir, 'inputs.json.gz'))));
    if (frames.length !== setup.frames) problems.push(`frame count ${frames.length} vs ${setup.frames}`);
    rows = JSON.parse(await checkRows(args, frames));
    if (rows.differences) problems.push(`tick rows: ${rows.differences} differ, first ${JSON.stringify(rows.first)}`);
    results.push({ label, args, problems, assets: native.assets.length, assetsRecorded: Object.keys(recorded).length, bodiesAhead: ahead, sameOrder, rows: rows.rows, rowDifferences: rows.differences, first: rows.first });
  } else results.push({ label, args, problems });
  if (problems.length) failures++;
  console.log(`${problems.length ? 'DIFF ' : 'ok   '}${label.padEnd(28)} setup ${native?.worldSetup ?? '-'} assets ${native?.assets.length ?? '-'}${native ? ` (${results.at(-1).bodiesAhead} bodies ahead${results.at(-1).sameOrder ? '' : ', order differs'})` : ''} rows ${rows?.rows ?? '-'}${problems.length ? '\n     ' + problems.join('\n     ') : ''}`);
}
const summary = { corpus, content, driver, cases: results.length, failures, rows: results.reduce((n, r) => n + (r.rows || 0), 0), results };
if (out) fs.writeFileSync(out, JSON.stringify(summary, null, 1) + '\n');
console.log(`${results.length} battles, ${summary.rows} tick rows, ${failures} with differences`);
process.exit(failures ? 1 : 0);
