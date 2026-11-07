import { bridgeFor } from '../sim/runtime.js';
// Cold provider attachment; transport receives a backend-neutral facade.
export function createWireCosmeticsBridge(bridge,...args) { return bridge.backend.prepare('createWireCosmeticsBridge',bridge,...args); }
export function createWireCosmetics(source,...args) { return createWireCosmeticsBridge(bridgeFor(source),...args); }
