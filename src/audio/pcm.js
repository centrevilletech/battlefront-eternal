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
export async function scalePcm(buffer, value, decibels = true) {
  if (!value && decibels) return buffer;
  await prepareAudioPcm();
  const gain = decibels ? x.asset_gain(value) : value;
  for (let c = 0; c < buffer.numberOfChannels; c++) {
    const data = buffer.getChannelData(c), at = x.asset_pcm(data.length);
    new Float32Array(x.memory.buffer, at, data.length).set(data);
    x.asset_scale(gain);
    data.set(new Float32Array(x.memory.buffer, at, data.length));
  }
  return buffer;
}
