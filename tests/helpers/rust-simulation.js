import { simModule } from '../../src/sim/module.js';
import { IMPORT_SYMBOLS } from '../../src/sim/gen/imports.js';
import { SimBridge } from '../../src/sim/runtime.js';
import { Ordnance } from '../../src/ordnance.js';
import { KIND_ITEM, KIND_ITEMS } from '../../src/sim/gen/index.js';
import { Items } from '../../src/items.js';
import { attachOrdnanceSim, ordnanceTests } from '../../src/ordnance-sim.js';
import { attachItemsSim } from '../../src/items-sim.js';

const module = await simModule();
function bridge() {
  const holder = { bridge: null }, bfe = {};
  for (const imp of IMPORT_SYMBOLS) if (imp) bfe[imp.symbol] = () => holder.bridge.serve(imp.code);
  const instance = new WebAssembly.Instance(module, { bfe });
  instance.holder = holder;
  return new SimBridge(instance, { tests: { ...ordnanceTests,
    emitter: value => typeof value?.viewDirection === 'function',
    unit: value => value != null && !['Vehicle','Turret'].includes(value.constructor?.name) && ('generation' in value || 'team' in value),
    vehicle: value => value?.constructor?.name === 'Vehicle',
    turret: value => value?.constructor?.name === 'Turret',
  } });
}

export function rustOrdnance(options) {
  const ordnance = new Ordnance(options);
  attachOrdnanceSim(ordnance, bridge());
  ordnance.sim.nativeServices = {
    owner(){},ordnance(){},explosion(){},item(){},itemTargets(){},
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
