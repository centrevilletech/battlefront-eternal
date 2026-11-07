// A server supplies this factory to createGame before any store/ray/pose
// attachment. Each battle still receives its own Rust WASM instance.
import { loadWasmBackend } from './wasm/backend.js';
import { HOST_SCHEMA } from './host-schema.js';
export function hostSimulation(manifest) {
  if (manifest.schema !== HOST_SCHEMA) throw Error('Host schema mismatch');
  if (manifest.worldBackend !== 'wasm') throw Error('Native world operations unavailable');
  return loadWasmBackend;
}
