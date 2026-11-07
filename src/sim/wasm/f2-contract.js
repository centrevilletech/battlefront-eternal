// Share the ownership mirror detector across native phase boundaries.
// A coordinated owner supplies the shared crossing counters separately.
import { MirrorWriteCheck } from '../phase-contract.js';

export class F2MirrorCheck extends MirrorWriteCheck {
  constructor() { super('F2'); }
}
