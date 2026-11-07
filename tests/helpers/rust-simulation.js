import { simModule, simImports } from '../../src/sim/module.js';
import { SimBridge } from '../../src/sim/runtime.js';
import { Ordnance } from '../../src/ordnance.js';
import { KIND_ITEM, KIND_ITEMS } from '../../src/sim/gen/index.js';
import { Items } from '../../src/items.js';
import { attachOrdnanceSim, ordnanceTests } from '../../src/ordnance-sim.js';
import { attachItemsSim } from '../../src/items-sim.js';
import { presentationLoaded } from './presentation.js';

const module = await simModule();
function bridge() {
  const holder = { bridge: null };
  const instance = new WebAssembly.Instance(module, simImports(holder));
  instance.holder = holder;
  return new SimBridge(instance, { tests: { ...ordnanceTests,
    emitter: value => typeof value?.viewDirection === 'function',
    unit: value => value != null && !['Vehicle','Turret'].includes(value.constructor?.name) && ('generation' in value || 'team' in value),
    vehicle: value => value?.constructor?.name === 'Vehicle',
    turret: value => value?.constructor?.name === 'Turret',
  } });
}

export function rustOrdnance(options) {
  const ordnance = new Ordnance({ presentation: presentationLoaded, ...options });
  attachOrdnanceSim(ordnance, bridge());
  ordnance.sim.nativeServices = {
    owner(){},ordnance(){},explosion(){},item(){},
    beforeStep(){},afterStep(){},
    explode(def,position,owner,visual) {
      const wet=ordnance.presentExplosion(visual??def,position);
      ordnance.onExplode(def,position.clone(),owner,{wet});
      ordnance.onExplosion(def,position,owner,{wet});
    },
  };
  const update = ordnance.update.bind(ordnance);
  ordnance.update = (dt, draw = true) => { update(dt); if (draw) ordnance.present(); };
  return ordnance;
}

export function rustItems(options) {
  const items = new Items(options);
  const rounds = rustOrdnance({ ...options, texture: () => null });
  attachItemsSim(items, rounds.sim);
  rounds.sim.nativeServices.afterStep=()=>{
    items.sim.bridge.sync(KIND_ITEM);
    items.sim.bridge.sync(KIND_ITEMS,items);
  };
  return items;
}
