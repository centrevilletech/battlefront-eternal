// WorldCorpus instruments raw module exports. The game's factory now takes a
// concrete backend, so this fixture wraps the instrumented instance before the
// battle attaches its cold owners. Recording still runs the original harness.
import {WorldCorpus} from '../../../tools/world-corpus.mjs';
const instrument = WorldCorpus.prototype.simulationFactory;
WorldCorpus.prototype.simulationFactory = async function(load) {
  const instance = await instrument.call(this, load);
  const {wasmBackendFor} = await import('../../../src/sim/wasm/backend.js');
  const {bridgeFor} = await import('../../../src/sim/runtime.js');
  const backend = wasmBackendFor(instance);
  try {
    bridgeFor(instance);
    const {attachments} = await import('../../../src/sim/wasm/attachments.js');
    backend.attachments = {...backend.attachments, ...attachments};
    return backend;
  } catch (error) {backend.close(); throw error;}
};
await import('../../../tools/world-hash.mjs');
