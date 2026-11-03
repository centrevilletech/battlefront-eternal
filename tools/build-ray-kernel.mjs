// The simulation's module alone (the ray kernel's, since rust/sim took in
// rust/rays): the same as `node tools/build-rust.mjs sim`, which builds
// rust/sim for WebAssembly and writes src/sim-wasm.js (add --check to
// compare with the committed file instead).
import { cli } from './build-rust.mjs';

cli(['sim', ...process.argv.slice(2)]);
