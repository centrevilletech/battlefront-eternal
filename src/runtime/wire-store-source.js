// Canonical capture belongs to the selected world's provider.
export function createStoreSource(game,...args) { return game.simBridge.backend.prepare('createStoreSource',game,...args); }
