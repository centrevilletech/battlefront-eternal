import { bridgeFor } from '../sim/runtime.js';
// The provider owns the session; transport retains only its stable facade.
export class WireEntityEdges {
  constructor(options) {
    const bridge=options.bridge ?? bridgeFor(options.instance);
    return bridge.backend.prepare('WireEntityEdges',{...options,bridge});
  }
}
