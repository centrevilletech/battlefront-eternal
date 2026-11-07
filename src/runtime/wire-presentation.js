import { bridgeFor } from '../sim/runtime.js';
// Cold provider attachment; transport receives a backend-neutral facade.
export function createWirePresentationBridge(bridge,...args) { return bridge.backend.prepare('createWirePresentationBridge',bridge,...args); }
export function createWirePresentation(source,...args) { return createWirePresentationBridge(bridgeFor(source),...args); }
