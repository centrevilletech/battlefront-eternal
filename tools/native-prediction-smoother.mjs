// Synchronous diagnostics drive the production frame owner directly. A live
// page instead drains both correction queues in its one network present call.
import {CorrectionSmoother as View} from '../src/runtime/prediction-view.js';
import {createPageNetwork} from '../src/runtime/page-network.js';
export class CorrectionSmoother extends View{
  constructor(options){super(options);this.owner=createPageNetwork();this.owner.previewSetup();}
  flush(){this.prepare(this.owner,0);this.owner.present(0);this.publish(this.owner,0);}
  note(...facts){super.note(...facts);this.flush();return this.lastSize;}
  decay(dt){super.decay(dt);this.flush();}
  reset(){super.reset();this.flush();}
  dispose(){this.owner.dispose();}
}
