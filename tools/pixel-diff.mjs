// Pixel difference of two .rgba captures from tools/render-bench-cdp.mjs:
// node tools/pixel-diff.mjs before-rest.rgba after-rest.rgba [diff.png]
import fs from 'node:fs';
const [a, b, outPng] = process.argv.slice(2);
const load = (f) => { const buf = fs.readFileSync(f); const w = buf.readUInt32LE(0), h = buf.readUInt32LE(4); return { w, h, data: buf.subarray(8) }; };
const A = load(a), B = load(b);
if (A.w !== B.w || A.h !== B.h) { console.log('size mismatch', A.w, A.h, B.w, B.h); process.exit(1); }
let changed = 0, big = 0, sum = 0, max = 0; const n = A.w * A.h;
const diff = Buffer.alloc(n * 4);
for (let i = 0; i < n; i++) {
  let d = 0; for (let c = 0; c < 3; c++) d = Math.max(d, Math.abs(A.data[i * 4 + c] - B.data[i * 4 + c]));
  if (d > 0) changed++; if (d > 8) big++; sum += d; if (d > max) max = d;
  diff[i * 4] = Math.min(255, d * 4); diff[i * 4 + 1] = d > 8 ? 255 : 0; diff[i * 4 + 2] = 0; diff[i * 4 + 3] = 255;
}
console.log(`${a.split('/').pop()} vs ${b.split('/').pop()}: changed ${changed} / ${n} (${(100 * changed / n).toFixed(2)}%), >8 levels: ${big} (${(100 * big / n).toFixed(2)}%), mean ${(sum / n).toFixed(3)}, max ${max}`);
if (outPng) {
  const zlib = await import('node:zlib');
  const w = A.w, h = A.h;
  const raw = Buffer.alloc((w * 4 + 1) * h);
  for (let y = 0; y < h; y++) { raw[y * (w * 4 + 1)] = 0; diff.copy(raw, y * (w * 4 + 1) + 1, (h - 1 - y) * w * 4, (h - y) * w * 4); }
  const crc = (buf) => { let c, crc = 0xffffffff; for (let n = 0; n < buf.length; n++) { c = (crc ^ buf[n]) & 0xff; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; crc = (crc >>> 8) ^ c; } return (crc ^ 0xffffffff) >>> 0; };
  const chunk = (type, body) => { const len = Buffer.alloc(4); len.writeUInt32BE(body.length); const tb = Buffer.concat([Buffer.from(type), body]); const c = Buffer.alloc(4); c.writeUInt32BE(crc(tb)); return Buffer.concat([len, tb, c]); };
  const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 6;
  fs.writeFileSync(outPng, Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', zlib.deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]));
}
