import test from "node:test";
import assert from "node:assert/strict";
import { fnv1a, hashKey } from "../src/audio/hash.js";
import { decodeXboxAdpcm, decodeSubstreams } from "../src/audio/adpcm.js";
import { LAYOUTS, autoLayout } from "../src/audio/layouts.js";

// A tiny Xbox ADPCM encoder for test signals: the reference IMA step and
// index rules, 64 frames per 36-byte block (header sample, 63 codes).
const STEPS = [7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767];
const INDEX = [-1, -1, -1, -1, 2, 4, 6, 8];
function encode(channels) {
  const frames = channels[0].length;
  const blocks = Math.floor(frames / 64);
  const out = new Uint8Array(blocks * 36 * channels.length);
  const state = channels.map(() => ({ pred: 0, index: 0 }));
  for (let b = 0; b < blocks; b++) {
    const base = b * 36 * channels.length;
    channels.forEach((ch, c) => {
      const s = state[c];
      s.pred = Math.max(-32768, Math.min(32767, Math.round(ch[b * 64])));
      const h = base + 4 * c;
      out[h] = s.pred & 0xff;
      out[h + 1] = (s.pred >> 8) & 0xff;
      out[h + 2] = s.index;
      const codes = [];
      for (let k = 1; k < 64; k++) {
        const target = ch[b * 64 + k];
        const step = STEPS[s.index];
        let diff = target - s.pred;
        let code = 0;
        if (diff < 0) {
          code = 8;
          diff = -diff;
        }
        let vp = step >> 3;
        if (diff >= step) { code |= 4; diff -= step; vp += step; }
        if (diff >= step >> 1) { code |= 2; diff -= step >> 1; vp += step >> 1; }
        if (diff >= step >> 2) { code |= 1; vp += step >> 2; }
        s.pred = Math.max(-32768, Math.min(32767, code & 8 ? s.pred - vp : s.pred + vp));
        s.index = Math.max(0, Math.min(88, s.index + INDEX[code & 7]));
        codes.push(code);
      }
      codes.push(0);
      for (let g = 0; g < 8; g++) {
        for (let k = 0; k < 4; k++) {
          const i = g * 8 + k * 2;
          out[base + 4 * channels.length + (g * channels.length + c) * 4 + k] = codes[i] | (codes[i + 1] << 4);
        }
      }
    });
  }
  return out;
}


test("names hash as the engine hashes them (each byte OR 0x20)", () => {
  assert.equal(hashKey("SoundProperties"), "0xee65c519");
  assert.equal(hashKey("MinDistance"), "0xb835cf50");
  // "_" becomes 0x7f, so this isn't the lowercase name's FNV-1a.
  assert.equal(fnv1a("Value_ATK_CIS"), 0x00e73245);
  assert.equal(hashKey("nab2cw"), "0xeb76afde");
});

test("Xbox ADPCM decodes a stereo test tone back to the tone", () => {
  const n = 64 * 40;
  const L = Float64Array.from({ length: n }, (_, i) => 12000 * Math.sin((2 * Math.PI * 440 * i) / 48000));
  const R = Float64Array.from({ length: n }, (_, i) => 8000 * Math.sin((2 * Math.PI * 660 * i) / 48000 + 1));
  const [dl, dr] = decodeXboxAdpcm(encode([L, R]), 2);
  assert.equal(dl.length, n);
  let err = 0;
  let pow = 0;
  for (let i = 64; i < n; i++) {
    err += (dl[i] * 32768 - L[i]) ** 2 + (dr[i] * 32768 - R[i]) ** 2;
    pow += L[i] ** 2 + R[i] ** 2;
  }
  // 4-bit ADPCM on a clean tone: better than 25 dB SNR.
  assert.ok(10 * Math.log10(pow / err) > 25, `SNR ${10 * Math.log10(pow / err)}`);
  // The header sample is the block's first frame.
  assert.equal(Math.round(dl[64] * 32768), Math.round(L[64]));
});

test("substreams interleave in fixed chunks and come back in order", () => {
  const n = 64 * 32;
  const make = (f) => Float64Array.from({ length: n }, (_, i) => 9000 * Math.sin((2 * Math.PI * f * i) / 44100));
  const front = encode([make(300), make(310)]);
  const rear = encode([make(500), make(510)]);
  const chunk = 72 * 8; // 8 blocks per chunk
  const mixed = new Uint8Array(front.length + rear.length);
  for (let k = 0, at = 0; k * chunk < front.length; k++) {
    mixed.set(front.subarray(k * chunk, (k + 1) * chunk), at);
    at += chunk;
    mixed.set(rear.subarray(k * chunk, (k + 1) * chunk), at);
    at += chunk;
  }
  const out = decodeSubstreams(mixed, 2, 2, chunk);
  assert.equal(out.length, 4);
  const ref = decodeXboxAdpcm(rear, 2);
  for (let i = 0; i < n; i += 97) assert.equal(out[2][i], ref[0][i]);
});

test("auto layout follows the device's channel count", () => {
  assert.equal(autoLayout(2), "stereo");
  assert.equal(autoLayout(6), "5.1");
  assert.equal(autoLayout(8), "7.1");
  assert.equal(autoLayout(12), "7.1.4");
});
