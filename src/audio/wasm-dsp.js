// The audio worklets' DSP in WebAssembly (rust/audio), which the game
// runs: the module compiled on the page and handed to each worklet
// processor through its processorOptions (the worklet scope cannot fetch,
// and a WebAssembly.Module can be sent to it). ?audio=js keeps the
// worklets' JavaScript instead, a developer's A/B switch: the two compute
// the same samples to the bit (tests/audio-dsp-parity.mjs offline), so it
// changes how they are computed, never what they are. A processor that
// gets no module, or cannot instantiate it, runs its JavaScript.
import { AUDIO_DSP_WASM } from "../audio-dsp-wasm.js";

let compiling = null;

// Whether this page runs the WebAssembly DSP: unless ?audio=js.
export function audioWasm() {
  try {
    return new URLSearchParams(globalThis.location?.search || "").get("audio") !== "js";
  } catch {
    return true;
  }
}

// Whether the voices render in the module's voice engine
// (rust/audio/src/voices.rs) rather than through a chain of the browser's
// nodes each, in the enhanced mode and the light mix: unless ?voices=native,
// a developer's A/B switch. The two give the same samples, to the bit
// where the browser's own are reproducible (docs/audio.md, "The voice
// engine"); the engine costs the audio thread about half as much.
export function voicesRust() {
  try {
    return new URLSearchParams(globalThis.location?.search || "").get("voices") !== "native";
  } catch {
    return true;
  }
}

// Whether the enhanced mode's master chain (the reverb, the LFE, the clarity
// stage, the limiter and the volume, after the mixer) renders in the mixer's
// module with its voices (rust/audio/src/master.rs) rather than in the
// browser's nodes and three more worklets: with ?master=rust, until it has
// shown what it saves on the low-end laptop.
export function masterRust() {
  try {
    return new URLSearchParams(globalThis.location?.search || "").get("master") === "rust";
  } catch {
    return false;
  }
}

// The compiled module, or null with ?audio=js (also null when it cannot be
// compiled: the JavaScript worklets then simply run, the same sound).
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
