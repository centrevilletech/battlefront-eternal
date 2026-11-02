// The ray kernel alone: the same as `node tools/build-rust.mjs rays`, which
// builds rust/rays for WebAssembly and writes src/ray-kernel-wasm.js
// (add --check to compare with the committed file instead).
import { cli } from './build-rust.mjs';

cli(['rays', ...process.argv.slice(2)]);
