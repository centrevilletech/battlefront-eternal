// The Rust CI job supplies the instrumented binary. Content-free real Context
// tests run independently of the long game-data qualification tools.
import test from 'node:test';
import {fileURLToPath} from 'node:url';
import {qualifyAddon} from '../tools/host-qualification/addon.mjs';
test('Context resources release after failed construction, GC and active worker termination',async()=>{
  await qualifyAddon(fileURLToPath(new URL('../.host-addon/bfe-node.node',import.meta.url)));
});
