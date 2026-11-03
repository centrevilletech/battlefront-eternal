// Differential inputs and exact trace for rust/content's existing policy
// and query parser. No game files needed; the JavaScript policy is the
// reference. Native Policy is the loader projection (screen, side, class,
// team, post), not the runtime screen's derived `auto` object.
//
// node tools/spawn-policy-twin.mjs --out DIR
// cargo run --release --manifest-path rust/Cargo.toml -p content \
//   --bin spawn-policy-twin -- DIR/ops.jsonl DIR/rust.txt
// node tools/spawn-policy-twin.mjs --out DIR --check
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { spawnPolicy, playableClasses, teamOfClass, sideOfClass } from '../src/spawn-screen/policy.js';

const args = process.argv.slice(2);
const option = (key, fallback) => { const i = args.indexOf(`--${key}`); return i < 0 ? fallback : args[i + 1]; };
const out = option('out', null);
if (!out) throw Error('spawn-policy-twin: --out DIR');
if (args.includes('--check')) {
  const expected = fs.readFileSync(path.join(out, 'js.txt'), 'utf8').trimEnd().split('\n');
  const actual = fs.readFileSync(path.join(out, 'rust.txt'), 'utf8').trimEnd().split('\n');
  const ops = fs.readFileSync(path.join(out, 'ops.jsonl'), 'utf8').trimEnd().split('\n');
  const different = [];
  for (let i = 0; i < Math.max(expected.length, actual.length); i++) if (expected[i] !== actual[i]) different.push(i);
  for (const i of different.slice(0, 12)) console.error(`row ${i}: ${ops[i]}\nJS ${expected[i]}\nRust ${actual[i]}`);
  assert.equal(different.length, 0, `${different.length} of ${expected.length} operations differ`);
  console.log(`spawn-policy-twin: ${expected.length} operations identical`);
  process.exit(0);
}

const text = s => Array.from({ length: s.length }, (_, i) => s.charCodeAt(i).toString(16).padStart(4, '0')).join('');
const buffer = new DataView(new ArrayBuffer(8));
const number = n => { if (Number.isNaN(n)) return 'nan'; buffer.setFloat64(0, n); return buffer.getBigUint64(0).toString(16).padStart(16, '0'); };
const rows = [], trace = [], counts = {};
const add = (op, fields, result) => {
  trace.push(`${rows.length}\t${result}`);
  rows.push({ op, ...fields });
  counts[op] = (counts[op] ?? 0) + 1;
};
const policy = (query, mission) => {
  // The native API accepts URL text. Plain objects are the source reader's
  // nullish-to-empty / String conversion, then host-encoded as URL text.
  const serialized = typeof query === 'string' ? query.toWellFormed() : query instanceof URLSearchParams ? query.toString()
    : new URLSearchParams(Object.entries(query ?? {}).map(([key, value]) => [key, value == null ? '' : String(value)])).toString();
  const p = spawnPolicy(query, mission);
  add('policy', { query: serialized, mission }, `${+p.screen} ${text(p.side)} ${text(p.className)} ${number(p.team)} ${p.post === null ? '-' : text(p.post)}`);
};
const team = classes => ({ classes: classes.map(name => [name, 1]) });
const missions = [
  null, {}, { teams: {} }, { sides: [], teams: {} },
  { sides: ['rep', 'cis'], teams: { 2: { ...team(['rep_hero', 'rep_soldier', 'rep_inf_jet_trooper']), hero: 'rep_hero' }, 1: team(['cis_soldier', 'cis_droideka']) } },
  { sides: ['all', 'imp'], teams: { 2: { ...team(['all_hero', 'all_soldier']), hero: 'all_hero' }, 1: team(['imp_soldier']) } },
  { sides: ['gar', 'cis'], teams: { 2: team(['gar_soldier']), 1: team(['cis_soldier']) } },
  { teams: { 2: { ...team(['rep_hero']), hero: 'rep_hero' }, 1: { classes: [[null], [false], [0], [''], ['REP_Soldier']], hero: 'rep_soldier' } } },
  { sides: ['REP', 'cis'], teams: { 10: team(['rep_later', 'shared']), 2: team(['rep_first', 'shared']), 1: null } },
  { teams: { '01': team(['rep_inf_jet_trooper', 'rep_alias']), '1': team(['rep_canonical']), '3': team(['rep_later']) } },
  { teams: { '-0': team(['rep_inf_jet_trooper']), '0': team(['rep_zero']), '2e0': team(['cis_exponent']), '2': team(['cis_canonical']) } },
];
const networks = [undefined, '', '0', '1', 'false'];
const sides = [undefined, '', 'rep', 'REP', 'ReP', 'cis', 'all', 'imp', 'gar', 'unknown'];
const units = [undefined, '', 'rep_inf_jet_trooper', 'rep_hero', 'cis_droideka', 'REP_Soldier', '_unit', 'unknown'];
const spawns = [undefined, '', 'CP3'];
const forced = [undefined, '', '0', '1', 'false'];
for (const mission of missions) {
  for (const network of networks) for (const side of sides) for (const unit of units) for (const spawn of spawns) for (const spawnscreen of forced) {
    const query = Object.fromEntries(Object.entries({ network, side, unit, spawn, spawnscreen }).filter(([, value]) => value !== undefined));
    policy(query, mission);
  }
  for (const value of [null, false, true, 0, -0, 1, [], ['rep'], ['rep', 'cis']]) {
    for (const key of ['network', 'side', 'unit', 'spawn', 'spawnscreen']) policy({ [key]: value }, mission);
  }
  for (const t of Object.values(mission?.teams ?? {})) add('playable', { team: t }, playableClasses(t).map(text).join(','));
  for (const className of [...units.filter(v => v !== undefined), 'shared', 'rep_first', 'cis_exponent']) {
    add('team', { mission, className }, number(teamOfClass(mission, className)));
    for (const fallback of ['', 'rep', 'CUSTOM']) add('side', { mission, className, fallback }, text(sideOfClass(mission, className, fallback)));
  }
}

// Text is kept exact, including lowercasing that expands and context-sensitive
// Greek sigma. Every Unicode scalar changed by Node's lowercasing is covered.
const names = ['ΟΣ', 'ΟΣΑ', 'ΟΣ\u0301', 'AΣ\u0301', 'Σ', 'İ', 'ẞ', 'K', '𐐀', '𐐀Σ', '😀', '\0', 'A-B'];
for (let cp = 0; cp <= 0x10ffff; cp++) {
  if (cp >= 0xd800 && cp <= 0xdfff) continue;
  const s = String.fromCodePoint(cp);
  if (s.toLowerCase() !== s) names.push(s);
}
for (const prefix of names) {
  const className = `${prefix}_Unit`, side = prefix.toLowerCase();
  const mission = { sides: [side], teams: { 2: team([className]) } };
  policy({ side: prefix }, mission);
  policy({ unit: className }, mission);
  add('side', { mission, className, fallback: 'MISS' }, text(sideOfClass(mission, className, 'MISS')));
}

// URLSearchParams keeps the first duplicate, changes '+' to a space, and
// replaces invalid UTF-8. Query::parse must do the same.
const urls = ['?', '', '?network=0&unit=cis_droideka&spawn=CP3', 'side=rep&side=cis', 'side=&side=cis',
  '?%73ide=REP&spawn=x+y', 'side=rep&&unit=rep_hero&', 'network=&network=1', 'spawnscreen=0&spawnscreen=1',
  ...['%', '%2', '%zz', '%C0%AF', '%E0%A4%A', '%ED%A0%80', '%F4%90%80%80', '%E2%82%AC', '+', '%00', '%26', '%3d', 'é', '%ef%bb%bf', '\ud800'].map(value => `unit=${value}&spawn=${value}&side=${value}`)];
for (const query of urls) {
  for (const mission of [null, missions[4]]) {
    policy(query, mission);
    policy(new URLSearchParams(query), mission);
  }
  for (const key of ['network', 'side', 'unit', 'spawn', 'spawnscreen']) add('query', { query: query.toWellFormed(), key }, text(new URLSearchParams(query).get(key) || ''));
}

// Number(team key) may be NaN, or two distinct keys may name the same
// number. The subtraction comparator then is not a total order. These
// cases exercise V8's sort order beyond its small-array insertion pass.
let random = 12345;
const next = () => ((random = (Math.imul(random, 1664525) + 1013904223) >>> 0) / 4294967296);
for (let i = 0; i < 1000; i++) {
  const keys = Array.from({ length: 80 }, (_, k) => next() < 0.25 ? `bad${k}` : `${Math.floor(next() * 100)}.0`);
  const mission = { teams: Object.fromEntries(keys.map((key, k) => [key, team([k % 3 ? 'shared' : `other${k}`, `rep_${k}`, ...(k % 7 ? [] : ['rep_inf_jet_trooper'])])])) };
  add('team', { mission, className: 'shared' }, number(teamOfClass(mission, 'shared')));
  policy({ side: 'rep' }, mission);
  policy({}, mission);
}
const edgeTeams = { teams: Object.fromEntries(['Infinity', 'bad', '-Infinity', 'NaN', '', '0x20', ' 2 ', '-0', '0', '2', '2.0', '9007199254740993', '9007199254740992']
  .map((key, i) => [key, team(['shared', `rep_${i}`, 'rep_inf_jet_trooper'])])) };
add('team', { mission: edgeTeams, className: 'shared' }, number(teamOfClass(edgeTeams, 'shared')));
policy({ side: 'rep' }, edgeTeams);
policy({}, edgeTeams);

fs.mkdirSync(out, { recursive: true });
fs.writeFileSync(path.join(out, 'ops.jsonl'), rows.map(row => JSON.stringify(row)).join('\n') + '\n');
fs.writeFileSync(path.join(out, 'js.txt'), trace.join('\n') + '\n');
fs.writeFileSync(path.join(out, 'summary.json'), JSON.stringify({ operations: rows.length, counts, unicode: process.versions.unicode,
  boundary: 'URL text, or plain query values host-stringified and encoded; mission class/hero/side names are Unicode scalar strings; team keys include noncanonical numbers and NaN conversions',
  excluded: ['lone surrogates in mission JSON (content parser rejects them)', 'non-string truthy class names', 'custom query getters/proxies'] }, null, 2) + '\n');
console.log(`spawn-policy-twin: ${rows.length} operations ${JSON.stringify(counts)}; Node Unicode ${process.versions.unicode}`);
