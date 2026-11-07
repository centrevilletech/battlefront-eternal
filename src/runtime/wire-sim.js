import { bridgeFor } from '../sim/runtime.js';
// Cold provider attachment; transport receives a backend-neutral facade.
export function createWireBridge(bridge,...args) { return bridge.backend.prepare('createWireBridge',bridge,...args); }
export function createWireSim(source,...args) { return createWireBridge(bridgeFor(source),...args); }
