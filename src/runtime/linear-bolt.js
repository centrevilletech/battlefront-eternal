import {resourceHost} from '../ordnance-world.js';
import {OrdnanceHostOwner_VALUE as O,OrdnanceHostHelper_VALUE as H} from '../sim/gen/ordnance-host.js';
export function boltFade(def,after) {const resources=resourceHost();return resources.run(O.helper,H.boltFade,resources,def,after);}
