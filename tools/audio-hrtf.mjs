// Regenerate the checked-in HRTF PCM from Chromium's Composite.flac.
// Obtain that resource from the URL in rust/audio/data/README.md, then:
//   node tools/audio-hrtf.mjs path/to/Composite.flac [--check]
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
const path = process.argv[2];
if (!path || path.startsWith("--")) throw new Error("usage: node tools/audio-hrtf.mjs Composite.flac [--check]");
const pcm = execFileSync("ffmpeg", ["-v", "error", "-i", path, "-f", "s16le", "-acodec", "pcm_s16le", "pipe:1"], { maxBuffer: 1048576 });
if (pcm.length !== 24 * 10 * 256 * 2 * 2) throw new Error("unexpected HRTF resource dimensions");
const data = Buffer.alloc(pcm.length * 2);
// Chromium's signed-16 converter multiplies by float reciprocals. Using
// double division here changes the group-delay analysis of some kernels.
const positive = Math.fround(1 / 32767), negative = Math.fround(1 / 32768);
for (let i = 0; i < pcm.length / 2; i++) {
  const sample = pcm.readInt16LE(i * 2);
  data.writeFloatLE(sample * (sample >= 0 ? positive : negative), i * 4);
}
const dest = new URL("../rust/audio/data/irc-composite.f32", import.meta.url);
if (process.argv.includes("--check")) { if (!readFileSync(dest).equals(data)) throw new Error("HRTF PCM differs; regenerate it"); }
else writeFileSync(dest, data);
console.log(`HRTF PCM: ${data.length} bytes`);
