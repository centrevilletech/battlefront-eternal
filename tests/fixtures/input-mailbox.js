// Standalone transport fixtures own a native simulation explicitly. Battle
// mailboxes always borrow the already loaded world's bridge in production.
import { loadSim } from '../../src/sim/module.js';
import { bridgeFor } from '../../src/sim/runtime.js';
import { InputMailbox as Mailbox } from '../../src/runtime/input-mailbox.js';
const bridge=bridgeFor(await loadSim());
export class InputMailbox extends Mailbox {
  constructor(epoch=1,capacity=256,owner=bridge,registry) {super(epoch,capacity,owner,registry);}
}
