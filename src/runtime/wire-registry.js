// Cold provider attachment; transport receives a backend-neutral facade.
export function createWireRegistryBridge(bridge,...args) { return bridge.backend.prepare('createWireRegistryBridge',bridge,...args); }
