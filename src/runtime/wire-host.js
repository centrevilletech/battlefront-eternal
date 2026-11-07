import { bridgeFor } from '../sim/runtime.js';
// Cold provider attachment; transport receives a backend-neutral facade.
export function createWireBudgetBridge(bridge,...args) { return bridge.backend.prepare('createWireBudgetBridge',bridge,...args); }
export function createWireHostBridge(bridge,...args) { return bridge.backend.prepare('createWireHostBridge',bridge,...args); }
export function createWireStaggerBridge(bridge,...args) { return bridge.backend.prepare('createWireStaggerBridge',bridge,...args); }
export function createWireHost(source,...args) { return createWireHostBridge(bridgeFor(source),...args); }
export function createWireStagger(source,...args) { return createWireStaggerBridge(bridgeFor(source),...args); }
