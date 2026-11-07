import { bridgeFor } from '../sim/runtime.js';
// Cold provider attachment; transport receives a backend-neutral facade.
export function createWireSemanticBridge(bridge,...args) { return bridge.backend.prepare('createWireSemanticBridge',bridge,...args); }
export function createWireSemantic(source,...args) { return createWireSemanticBridge(bridgeFor(source),...args); }
