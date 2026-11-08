import {resourceHost,bindResources} from '../ordnance-world.js';
import {OrdnanceHostOwner_VALUE as O,OrdnanceHostClient_NAMES} from '../sim/gen/ordnance-host.js';
// These maps retain browser drawing resources. Native client::shots owns
// their identities, ordering, claims, cadence, flight and terminal lifetime.
export class PredictedShots {
  constructor(collision,now=()=>performance.now()/1000,ordnance) {
    this.collision=collision;this.now=now;this.ordnance=ordnance;
    this.shots=new Map();this.claims=new Map();this.views=new Map();
    this.resources=resourceHost(collision);this.resources.attach(this);
  }
  dispose() {this.clear();this.resources.release(this);}
}
bindResources(PredictedShots,O.client,OrdnanceHostClient_NAMES);
