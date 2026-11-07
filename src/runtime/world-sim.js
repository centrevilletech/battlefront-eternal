import { bridgeFor } from '../sim/runtime.js';
export function createWorldSim(source) { const bridge=bridgeFor(source); return bridge.backend.prepare('world'); }
