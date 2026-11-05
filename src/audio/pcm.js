import { loadAudioDsp } from "./wasm-dsp.js";

// Loading-time PCM preparation shares the required module. AudioBuffers
// are only host storage; normalization and playback compensation are Rust.
let x, preparing;
export function prepareAudioPcm() {
  preparing ||= loadAudioDsp().then((module) => { x = new WebAssembly.Instance(module).exports; });
  return preparing;
}
export function normalizePcm16(input, output) {
  if (!x) throw new Error("audio: Rust PCM preparation is not ready");
  const at = x.asset_i16(input.length);
  new Int16Array(x.memory.buffer, at, input.length).set(input);
  const result = x.asset_normalize();
  output.set(new Float32Array(x.memory.buffer, result, input.length));
}
