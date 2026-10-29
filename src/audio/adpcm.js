// The streams (music, voice-over, ambience) are Xbox IMA ADPCM, as the 2004
// PC game stored them (format 5 in the stream's Info). Per channel, 36 bytes
// hold 64 frames: a 4-byte header (int16 sample, step index, a pad byte),
// whose sample is the first frame, then 32 bytes of 4-bit codes, low nibble
// first, of which the last is unused. Channels interleave every 4 bytes. A
// stream with two substreams (Theed's ambience) interleaves two of these in
// fixed-size chunks.
const STEPS = new Int32Array([
  7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118,
  130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060,
  1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484,
  7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
]);
const INDEX_STEP = new Int8Array([-1, -1, -1, -1, 2, 4, 6, 8]);
export const BLOCK_BYTES = 36; // per channel
export const BLOCK_FRAMES = 64;

// Decode `bytes` (whole blocks of `channels` channels) into one Float32Array
// per channel, scaled to -1..1.
export function decodeXboxAdpcm(bytes, channels, out = null) {
  const block = BLOCK_BYTES * channels;
  const blocks = Math.floor(bytes.length / block);
  const frames = blocks * BLOCK_FRAMES;
  const result = out || Array.from({ length: channels }, () => new Float32Array(frames));
  const scale = 1 / 32768;
  for (let b = 0; b < blocks; b++) {
    const base = b * block;
    for (let c = 0; c < channels; c++) {
      const dest = result[c];
      let at = b * BLOCK_FRAMES;
      const h = base + 4 * c;
      let sample = (bytes[h] | (bytes[h + 1] << 8)) << 16 >> 16;
      let index = Math.min(bytes[h + 2], 88);
      dest[at++] = sample * scale;
      let count = 1;
      for (let group = 0; group < 8; group++) {
        const g = base + 4 * channels + (group * channels + c) * 4;
        for (let k = 0; k < 4; k++) {
          const byte = bytes[g + k];
          for (let n = 0; n < 2; n++) {
            if (count === BLOCK_FRAMES) break;
            const code = n === 0 ? byte & 15 : byte >> 4;
            const step = STEPS[index];
            let diff = step >> 3;
            if (code & 1) diff += step >> 2;
            if (code & 2) diff += step >> 1;
            if (code & 4) diff += step;
            sample = code & 8 ? sample - diff : sample + diff;
            if (sample < -32768) sample = -32768;
            else if (sample > 32767) sample = 32767;
            index += INDEX_STEP[code & 7];
            if (index < 0) index = 0;
            else if (index > 88) index = 88;
            dest[at++] = sample * scale;
            count++;
          }
        }
      }
    }
  }
  return result;
}

// A segment of a stream with several substreams: `bytes` is the stream's
// data from `offset` (a multiple of the interleave chunk), and the segment's
// share of each substream is size / count bytes. Returns every substream's
// channels in turn (front pair, then rear pair).
export function decodeSubstreams(bytes, channels, count, interleave) {
  if (count <= 1) return decodeXboxAdpcm(bytes, channels);
  const chunks = Math.floor(bytes.length / interleave);
  const parts = Array.from({ length: count }, () => new Uint8Array(Math.ceil(chunks / count) * interleave));
  const filled = new Array(count).fill(0);
  for (let k = 0; k < chunks; k++) {
    const p = k % count;
    parts[p].set(bytes.subarray(k * interleave, (k + 1) * interleave), filled[p]);
    filled[p] += interleave;
  }
  return parts.flatMap((part, p) => decodeXboxAdpcm(part.subarray(0, filled[p]), channels));
}
