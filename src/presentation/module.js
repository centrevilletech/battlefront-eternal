// Receive and display share this single module instance. A sender may exist
// before the battle's asset promises resolve, so setup is synchronous; the
// compiled bytes and retained memory are never recreated by a frame or packet.
import * as module from "../presentation-pkg/bfe_presentation.js";
import { PRESENTATION_WASM } from "../presentation-pkg/bytes.js";
// Resource loading and transport setup retain this same module owner.
let loaded;
export function presentationModule() {
  if (!loaded) {
    const bytes = Uint8Array.fromBase64
      ? Uint8Array.fromBase64(PRESENTATION_WASM)
      : Uint8Array.from(atob(PRESENTATION_WASM), (c) => c.charCodeAt(0));
    const wasm = module.initSync({ module: bytes });
    loaded = { module, memory: wasm.memory };
  }
  return loaded;
}
