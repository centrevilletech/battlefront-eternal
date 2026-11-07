// Host code shares identities and generated record I/O. Only raw WASM
// fixtures construct SimBridge directly; game factories supply a provider.
export * from './bridge-core.js';
export { SimBridge, StringTable } from './wasm/bridge.js';
import { SimBridge } from './wasm/bridge.js';
// The bridge of a battle's instance of the module: the one attached to it,
// or a new one. Every phase that goes onto the module shares it (the
// module's imports reach the bridge attached to the instance), adding what
// tells its kinds apart (`options`: classes, tests) and registering its own
// imports' handlers (SimBridge.register).
export function bridgeFor(instance, options = {}) {
  if (instance.getBridge) return instance.getBridge(options);
  const bridge = instance.holder?.bridge;
  return bridge ? bridge.identify(options) : new SimBridge(instance, options);
}
