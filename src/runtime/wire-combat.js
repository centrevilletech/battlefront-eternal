import { bridgeFor } from '../sim/runtime.js';
// The provider owns the session; transport retains only its stable facade.
export class WireCombat {
  constructor(options) {
    const bridge=options.bridge ?? bridgeFor(options.instance);
    return bridge.backend.prepare('WireCombat',{...options,bridge});
  }
}
