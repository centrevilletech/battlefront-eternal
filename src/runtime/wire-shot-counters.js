// Cold provider attachment; transport receives a backend-neutral facade.
export function createWireShotCounters(bridge,...args) { return bridge.backend.prepare('createWireShotCounters',bridge,...args); }
