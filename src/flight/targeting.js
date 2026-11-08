import {resourceHost} from '../ordnance-world.js';
import {OrdnanceHostOwner_VALUE as O,OrdnanceHostHelper_VALUE as H} from '../sim/gen/ordnance-host.js';
export function navigateMissile(round,target,dt) {const resources=resourceHost();return resources.run(O.helper,H.navigateMissile,resources,round,target,dt);}
