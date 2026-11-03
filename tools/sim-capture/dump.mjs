// Prints a capture's records (tools/sim-capture.mjs) as JSON lines, for
// looking into one:
//   node tools/sim-capture/dump.mjs FILE [--name ordnance.stepBolt] [--kind enter]
//     [--tick 700] [--limit 20] [--summary]
// --summary counts the records by name and kind instead.
import fs from 'node:fs';
import { readRecords, Handle, Omitted, FunctionName } from './format.mjs';

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i < 0 ? fallback : argv[i + 1]; };
const file = argv.find((a, i) => !a.startsWith('--') && !(i > 0 && argv[i - 1].startsWith('--') && argv[i - 1] !== '--summary'));
const name = opt('name', null), kind = opt('kind', null), tick = opt('tick', null), limit = Number(opt('limit', Infinity));
const show = (v) => JSON.stringify(v, (k, x) => {
  if (x instanceof Handle) return `<${x.kind} ${x.key}>`;
  if (x instanceof Omitted) return `<left out: ${x.why}>`;
  if (x instanceof FunctionName) return `<function ${x.name}>`;
  if (x instanceof Map) return { $map: [...x] };
  if (x instanceof Set) return { $set: [...x] };
  if (ArrayBuffer.isView(x)) return { $typed: x.constructor.name, values: [...x].slice(0, 16), length: x.length };
  if (typeof x === 'number' && !Number.isFinite(x)) return String(x);
  if (Object.is(x, -0)) return '-0';
  return x;
});
const summary = {};
let shown = 0;
for (const record of readRecords(new Uint8Array(fs.readFileSync(file)))) {
  const k = record.value?.k ?? (record.type === 1 ? 'header' : '?');
  if (argv.includes('--summary')) { const key = `${record.name || 'tick'} ${k}`; summary[key] = (summary[key] ?? 0) + 1; continue; }
  if ((name && record.name !== name) || (kind && k !== kind) || (tick && record.tick !== Number(tick))) continue;
  console.log(`${record.tick} ${record.name} ${show(record.value)}`);
  if (++shown >= limit) break;
}
if (argv.includes('--summary')) for (const [key, n] of Object.entries(summary)) console.log(`${n}\t${key}`);
