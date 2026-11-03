// The audio worklets' DSP in WebAssembly (rust/audio), for ?audio=wasm:
// the module compiled on the page and handed to each worklet processor
// through its processorOptions (the worklet scope cannot fetch, and a
// WebAssembly.Module can be sent to it). A processor that gets no module,
// or cannot instantiate it, runs its JavaScript as before, so the flag
// changes how the samples are computed, never what they are
// (tests/audio-dsp-parity.mjs holds the two to the same bits offline).
import { AUDIO_DSP_WASM } from "../audio-dsp-wasm.js";

let compiling = null;

// Whether this page asked for the WebAssembly DSP.
export function audioWasm() {
  try {
    return new URLSearchParams(globalThis.location?.search || "").get("audio") === "wasm";
  } catch {
    return false;
  }
}

// The compiled module when asked for, else null (also null when it cannot
// be compiled: the JavaScript worklets then simply run, the same sound).
export function loadAudioDsp() {
  if (!audioWasm()) return Promise.resolve(null);
  compiling ||= (async () => {
    const text = atob(AUDIO_DSP_WASM);
    const bytes = new Uint8Array(text.length);
    for (let i = 0; i < text.length; i++) bytes[i] = text.charCodeAt(i);
    return WebAssembly.compile(bytes);
  })().catch((error) => {
    console.warn(`audio: the WebAssembly DSP did not compile (${error.message}); using the JavaScript worklets`);
    return null;
  });
  return compiling;
}
