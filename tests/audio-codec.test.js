import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { gunzipSync } from "node:zlib";
import { decodeFixture } from "./audio-codec-host.js";
const fixture = (file) => readFileSync(new URL(`./fixtures/audio-codecs/${file}`, import.meta.url));
const manifest = JSON.parse(fixture("host.json"));
for (const scene of manifest.scenes) test(`Rust codec matches frozen host PCM: ${scene.name}`, () => {
  const decoded = decodeFixture(fixture(scene.file), { rate: scene.rate });
  assert.equal(decoded.rate, scene.rate); assert.equal(decoded.sourceRate, scene.sourceRate);
  assert.equal(decoded.channels.length, scene.channels); assert.equal(decoded.frames, scene.frames);
  const actual = Buffer.concat(decoded.channels.map((p) => Buffer.from(p.buffer)));
  const expected = gunzipSync(fixture(scene.name + ".f32.gz"));
  assert.equal(actual.length, expected.length);
  if (scene.codec !== "opus" && scene.rate === scene.sourceRate) assert.deepEqual(actual, expected);
  else {
    const a = new Float32Array(actual.buffer, actual.byteOffset, actual.length / 4);
    const b = new Float32Array(expected.buffer, expected.byteOffset, expected.length / 4);
    let error = 0;
    for (let i = 0; i < a.length; i++) error = Math.max(error, Math.abs(a[i] - b[i]));
    assert.ok(error < 10 ** (-96 / 20), `peak difference ${20 * Math.log10(error)} dBFS`);
  }
});
for (const file of ["flac24-stereo.flac", "opus-8ch.opus", "hrir-float16.wav"]) test(`codec input chunks do not change PCM: ${file}`, () => {
  const bytes = fixture(file), reference = decodeFixture(bytes, { rate: 44100 });
  for (const chunkSize of [7, 257, 4093]) {
    const actual = decodeFixture(bytes, { rate: 44100, chunkSize, maximum: 127 });
    assert.deepEqual(actual.channels, reference.channels);
  }
});
test("Rust preserves source rate unless conversion is explicitly requested", () => {
  const pcm = decodeFixture(fixture("flac16-22050.flac"));
  assert.equal(pcm.rate, 22050); assert.equal(pcm.frames, 8191);
});
test("HRIR gain compensation uses the source and requested rates", () => {
  const bytes = fixture("hrir-float16.wav");
  const plain = decodeFixture(bytes, { rate: 44100 });
  const scaled = decodeFixture(bytes, { rate: 44100, compensate: true });
  for (let c = 0; c < plain.channels.length; c++) assert.deepEqual(scaled.channels[c], Float32Array.from(plain.channels[c], (v) => v * (48000 / 44100)));
});
test("asset playback gain is applied after explicit rate conversion", () => {
  const bytes = fixture("flac24-stereo.flac"), gain = 6;
  const plain = decodeFixture(bytes, { rate: 44100 });
  const scaled = decodeFixture(bytes, { rate: 44100, gain });
  for (let c = 0; c < plain.channels.length; c++) assert.deepEqual(scaled.channels[c], Float32Array.from(plain.channels[c], (v) => v * 10 ** (gain / 20)));
});
test("corrupt and truncated containers fail without a host decoder", () => {
  for (const file of ["flac24-stereo.flac", "opus-8ch.opus", "hrir-float16.wav"]) {
    const bytes = fixture(file);
    assert.throws(() => decodeFixture(bytes.subarray(0, bytes.length - 80)));
  }
  const ogg = fixture("opus-6ch.opus"); ogg[ogg.length - 10] ^= 1;
  assert.throws(() => decodeFixture(ogg), /checksum|final page/);
});

test("Ogg packets continued across pages preserve PCM and end trim", () => {
  const bytes = fixture("opus-2ch.opus"), packets = [];
  let pending = [], endGranule, serial;
  for (let at = 0; at < bytes.length;) {
    const count = bytes[at + 26], table = bytes.subarray(at + 27, at + 27 + count);
    serial = bytes.readUInt32LE(at + 14); endGranule = bytes.readBigUInt64LE(at + 6);
    let offset = at + 27 + count;
    for (const n of table) {
      pending.push(bytes.subarray(offset, offset + n)); offset += n;
      if (n < 255) { packets.push(Buffer.concat(pending)); pending = []; }
    }
    at = offset;
  }
  let sequence = 0;
  const page = (data, flags, granule, partial = false) => {
    const table = partial ? [255] : [...Array(Math.floor(data.length / 255)).fill(255), data.length % 255];
    const header = Buffer.alloc(27 + table.length); header.write("OggS"); header[5] = flags;
    header.writeBigUInt64LE(granule, 6); header.writeUInt32LE(serial, 14); header.writeUInt32LE(sequence++, 18);
    header[26] = table.length; header.set(table, 27);
    const result = Buffer.concat([header, data]); let crc = 0;
    for (const byte of result) {
      crc = (crc ^ (byte << 24)) >>> 0;
      for (let bit = 0; bit < 8; bit++) crc = ((crc << 1) ^ (crc & 0x80000000 ? 0x04c11db7 : 0)) >>> 0;
    }
    result.writeUInt32LE(crc, 22); return result;
  };
  const pages = [];
  for (let i = 0; i < packets.length; i++) {
    const end = i === packets.length - 1;
    const granule = end ? endGranule : BigInt(Math.max(0, i - 1) * 960);
    if (i === 2) {
      assert.ok(packets[i].length > 255);
      pages.push(page(packets[i].subarray(0, 255), 0, 0xffffffffffffffffn, true));
      pages.push(page(packets[i].subarray(255), 1, granule));
    } else pages.push(page(packets[i], (i === 0 ? 2 : 0) | (end ? 4 : 0), i < 2 ? 0n : granule));
  }
  const continued = Buffer.concat(pages);
  for (const rate of [44100, 48000]) assert.deepEqual(decodeFixture(continued, { rate, chunkSize: 257 }).channels, decodeFixture(bytes, { rate }).channels);
});
