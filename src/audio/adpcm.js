import { AUDIO_DSP_WASM } from "../audio-dsp-wasm.js";

export const BLOCK_BYTES = 36;
export const BLOCK_FRAMES = 64;
let decoder;
function instance() {
  if (!decoder) {
    const bytes = Uint8Array.from(atob(AUDIO_DSP_WASM), (c) => c.charCodeAt(0));
    try { decoder = new WebAssembly.Instance(new WebAssembly.Module(bytes)).exports; }
    catch (cause) { const error = new Error(`audio: the required Rust stream decoder could not start: ${cause.message}`); error.audioModule = true; throw error; }
  }
  return decoder;
}
// Asset I/O in the stream worker. Rust owns the Xbox ADPCM rules and the
// substream deinterleave; only copied channel arrays cross the boundary.
export function decodeSubstreams(bytes, channels, count = 1, interleave = 0, out = null) {
  if (!Number.isInteger(channels) || channels < 1 || !Number.isInteger(count) || count < 1 || channels * count > 12 || (count > 1 && interleave < 1)) throw new Error("audio: invalid ADPCM channel layout");
  const x = instance();
  const at = x.adpcm_input(bytes.length);
  new Uint8Array(x.memory.buffer, at, bytes.length).set(bytes);
  const frames = x.adpcm_decode(channels, count, interleave);
  const result = out || Array.from({ length: channels * count }, () => new Float32Array(frames));
  for (let c = 0; c < result.length; c++) result[c].set(new Float32Array(x.memory.buffer, x.adpcm_output(c), frames));
  return result;
}
export function decodeXboxAdpcm(bytes, channels, out = null) { return decodeSubstreams(bytes, channels, 1, 0, out); }
