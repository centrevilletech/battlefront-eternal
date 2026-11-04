// Registry setup on the shared simulation; only foreign content references
// and the resulting JS lookup maps remain in the adapter.
import { WrResultMirror, syncWrResult, putWrInputs, putWrTables } from '../sim/gen/index.js';
const name = value => ({ value: typeof value === 'string' ? value : null });
export function createWireRegistryBridge(bridge) {
  const x = bridge.x, output = { Mirror: WrResultMirror, sync: syncWrResult };
  const inputs = [putWrInputs, putWrTables].map((put, op) => ({ put, words: x.wire_registry_words(op) }));
  function run(op, args) {
    bridge.callArgs(inputs[op], args); const out = bridge.callResult(output, x.wire_registry_run(op));
    if (out.error !== null) throw Error(out.error); return out;
  }
  return {
    inputs({ contentEpoch, inputCodes, bindingProfiles }) {
      const codesValid = Array.isArray(inputCodes) && inputCodes.length <= 128 && new Set(inputCodes).size === inputCodes.length;
      const profilesValid = bindingProfiles instanceof Map && bindingProfiles.size > 0 && bindingProfiles.size <= 256;
      const profiles = profilesValid ? [...bindingProfiles].map(([id, bindings]) => {
        const valid = !!bindings && typeof bindings === 'object' && !Array.isArray(bindings) && Object.keys(bindings).length <= 32;
        return { id: typeof id === 'symbol' ? NaN : Number(id), idValid: typeof id === 'number', valid,
          bindings: valid ? Object.keys(bindings).map(action => ({ action, valid: Array.isArray(bindings[action]), codes: Array.isArray(bindings[action]) ? bindings[action].map(name) : [] })) : [] };
      }) : [];
      const out = run(0, { epoch: typeof contentEpoch === 'number' ? contentEpoch : NaN, epochValid: typeof contentEpoch === 'number', codesValid, profilesValid,
        codes: codesValid ? inputCodes.map(name) : [], profiles });
      return { inputCodes: out.codes.map(code => code.value), profiles: new Map(out.profiles.map(p => {
        const value = {}; for (const binding of p.bindings) value[binding.action] = binding.codes.map(code => code.value);
        return [p.id, value];
      })) };
    },
    table(names) {
      const out = run(1, { classes: names.map(name), weapons: [], posts: [] });
      return new Map(out.classes.map((item, i) => [i + 1, item.value]));
    },
  };
}
