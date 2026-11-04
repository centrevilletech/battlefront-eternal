// Compare debris twin files without changing either trace. Finite cases
// must be byte-identical. Nonfinite cases also check every value with
// Object.is and preserve reference aliases; any NaN-bit differences are
// reported explicitly. V8 can change NaN signs inside the JS collision
// callback despite identical inputs when its caller is WebAssembly.
import fs from 'node:fs';
import path from 'node:path';
import { readRecords } from './sim-capture/format.mjs';
const [left, right] = process.argv.slice(2);
if (!left || !right) throw Error('debris-compare LEFT_DIRECTORY RIGHT_DIRECTORY');
const word = new DataView(new ArrayBuffer(8));
const bits = n => { word.setFloat64(0, n); return word.getBigUint64(0); };
let nanBits = 0;
function compare(a, b, at, seen = new Map(), back = new Map()) {
  if (typeof a === 'number' && typeof b === 'number') {
    if (!Object.is(a, b)) throw Error(`${at}: ${a} != ${b}`);
    if (bits(a) !== bits(b)) nanBits++;
    return;
  }
  if (a === null || b === null || typeof a !== 'object' || typeof b !== 'object') {
    if (!Object.is(a, b)) throw Error(`${at}: different value`); return;
  }
  if (seen.has(a)) { if (seen.get(a) !== b) throw Error(`${at}: different left alias`); return; }
  if (back.has(b)) throw Error(`${at}: different right alias`);
  seen.set(a, b); back.set(b, a);
  const keys = Reflect.ownKeys(a), other = Reflect.ownKeys(b);
  if (keys.length !== other.length || keys.some((key, i) => key !== other[i])) throw Error(`${at}: different keys`);
  for (const key of keys) compare(a[key], b[key], `${at}.${String(key)}`, seen, back);
}
const files = [];
for (const file of fs.readdirSync(left).filter(f => /^debris-\d+\.bfecap$/.test(f)).sort()) {
  const a = fs.readFileSync(path.join(left, file)), b = fs.readFileSync(path.join(right, file));
  const byteExact = a.equals(b), nonfinite = Number(file.match(/\d+/)[0]) >= 12, before = nanBits;
  if (!byteExact && !nonfinite) throw Error(`${file}: finite trace differs`);
  const aa = readRecords(a), bb = readRecords(b); let records = 0;
  for (;;) {
    const av = aa.next(), bv = bb.next();
    if (av.done || bv.done) { if (av.done !== bv.done) throw Error(`${file}: different record count`); break; }
    compare(av.value, bv.value, `${file}:${++records}`);
  }
  files.push({ file, byteExact, objectIsExact: true, nonfinite, records, nanBitDifferences: nanBits - before });
}
if (!files.length) throw Error('no debris traces');
console.log(JSON.stringify({ files, nanBitDifferences: nanBits }));
