// The audio module is required. Compilation is shared by the page's thin
// worklet hosts; failure is reported to the player, never routed elsewhere.
import { AUDIO_DSP_WASM } from "../audio-dsp-wasm.js";
let compiling = null;
export function loadAudioDsp() {
  compiling ||= (async () => {
    const text = atob(AUDIO_DSP_WASM);
    const bytes = new Uint8Array(text.length);
    for (let i = 0; i < text.length; i++) bytes[i] = text.charCodeAt(i);
    try { return await WebAssembly.compile(bytes); }
    catch (error) { throw new Error(`audio: the required Rust module could not load (${error.message}); reload the game`); }
  })();
  return compiling;
}
