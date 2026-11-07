// Cold attachments belong to the concrete provider. Game receives stable
// category facades and never needs a WASM instance or an export projection.
import { CollisionWorld } from '../../collision.js';
import { Unit } from '../../units.js';
import { Vehicle } from '../../vehicles.js';
import { loadRayKernel } from './ray-kernel.js';
import { loadAnimKernel } from './anim-kernel.js';
import { PageCameras } from './presentation/cameras.js';
import { missionTeams } from './mission-teams.js';
import { validateStartupClass } from './spawn-screen/startup.js';
import { loadPageWeaponPolicies, bindWeaponPolicyContent } from './weapon-policy-content.js';
import { attachF2 } from './f2-sim.js';
import { ClientHullContent } from './runtime/client-hull-content.js';
import { createNativeAI } from './ai-world-load.js';
import { F4Live } from './f4-live.js';
import { F4Tail } from './f4-tail.js';
import { createClientPrediction } from './runtime/client-prediction.js';
import { bridgeFor } from '../runtime.js';
// The battle's store on its instance of the module (src/sim/runtime.js
// SimBridge: the kinds Rust owns, the imports it asks the battle), with the
// rounds in flight and the things lying about handed over to it.
async function attachSimStore(instance, ordnance, items, profileBoundary) {
  const [{ bridgeFor }, { attachOrdnanceSim, ordnanceTests }, { attachItemsSim }] = await Promise.all([import("../runtime.js"), import("./ordnance-sim.js"), import("./items-sim.js")]);
  // The instance's one bridge (made here, or joined: another phase's own
  // attach gets the same one with bridgeFor).
  const bridge = bridgeFor(instance, { classes: { Unit, Vehicle }, tests: ordnanceTests });
  if (profileBoundary) {
    const { F4Counters, countedF4Exports } = await import('../../f4-counters.js');
    bridge.boundaryCounters = new F4Counters();
    bridge.x = countedF4Exports(bridge.x, bridge.boundaryCounters);
  }
  // Room for the store's rows now, so play does not grow the memory.
  bridge.reserve(4 << 20);
  attachItemsSim(items, attachOrdnanceSim(ordnance, bridge));
  return bridge;
}

// The persistent owner shares the battle instance and mixed target classes.
// A target subclass must be registered before a generated union sees it.
async function attachOwnedUnits(instance, game) {
  const [{attachF3},{Soldier},{Droideka},{Building,Turret},{VehicleTarget,TurretTarget},{ObjectiveTarget}] = await Promise.all([
    import('./f3-sim.js'),import('../../soldier.js'),import('../../droideka.js'),import('../../props.js'),
    import('../../seats.js'),import('../../objectives.js'),
  ]);
  return attachF3(instance,{Unit,Vehicle,Soldier,Droideka,Building,Turret,VehicleTarget,TurretTarget,ObjectiveTarget},game);
}

// The owning unit phase uses content-only combat construction. Live
// commands bind when UnitFactory finishes each body; exports stay intact.
async function attachOwnedCombat(instance) {
  const [{attachF3Combat},{Soldier},{Droideka},{Building,Turret}] = await Promise.all([
    import('./f3-combat.js'),import('../../soldier.js'),import('../../droideka.js'),import('../../props.js'),
  ]);
  return attachF3Combat(instance,{Unit,Vehicle,Soldier,Droideka,Building,Turret});
}


export const attachments = Object.freeze({
  collision: (backend,...args) => new CollisionWorld(...args),
  rays: backend => loadRayKernel(backend.instance),
  animation: backend => loadAnimKernel(backend.instance),
  cameras: (backend,...args) => new PageCameras(backend.instance,...args),
  teams: (backend,...args) => missionTeams(backend.instance,...args),
  startup: (backend,...args) => validateStartupClass(backend.instance,...args),
  pageWeapons: (backend,...args) => loadPageWeaponPolicies(backend.instance,...args),
  weaponPolicies: (backend,...args) => bindWeaponPolicyContent(backend.bridge,...args),
  store: (backend,...args) => attachSimStore(backend.instance,...args),
  combat: backend => attachOwnedCombat(backend.instance),
  units: (backend,...args) => attachOwnedUnits(backend.instance,...args),
  fleet: (backend,...args) => attachF2(args[0],bridgeFor(backend.instance,{classes:{Unit,Vehicle}}),...args.slice(1),Unit),
  hull: (backend,...args) => new ClientHullContent(args[0],bridgeFor(backend.instance,{classes:{Unit,Vehicle}}),...args.slice(1)),
  ai: (backend,...args) => createNativeAI(...args),
  live: (backend,...args) => new F4Live(...args),
  tail: (backend,...args) => new F4Tail(...args),
  prediction: (backend,...args) => createClientPrediction(backend.instance,...args),
});
