import { bridgeFor } from '../sim/runtime.js';
import { createActorPredictionBridge } from './actor-prediction-sim.js';
import { createWeaponPredictionBridge } from './weapon-prediction-sim.js';
import { createVehiclePredictionBridge } from './vehicle-prediction-sim.js';
import { createRemotePredictionBridge } from './remote-prediction-sim.js';
import { createCorrectionBridge } from './correction-sim.js';
import { createReconcileBridge } from './reconcile-sim.js';

// The page owns these sessions for one battle. Short-lived replay wrappers
// borrow them; closing a connection releases the cached body owners as well.
export function createPagePrediction(instance, collision) {
  const bridge = bridgeFor(instance), owners = [];
  const keep = owner => { owners.push(owner); return owner; };
  let disposed = false;
  function dispose() {
    if (disposed) return;
    disposed = true;
    let failure, failed = false;
    for (const owner of owners) {
      try { owner.dispose(); } catch (error) { if (!failed) { failure = error; failed = true; } }
    }
    if (failed) throw failure;
  }
  try {
    const actors = keep(createActorPredictionBridge(bridge, collision));
    const weapons = keep(createWeaponPredictionBridge(bridge));
    const vehicles = keep(createVehiclePredictionBridge(bridge));
    const remotes = keep(createRemotePredictionBridge(bridge));
    const corrections = keep(createCorrectionBridge(bridge));
    const reconcile = keep(createReconcileBridge(bridge));
    return { bridge, actors, weapons, vehicles, remotes, corrections, reconcile, dispose, get disposed() { return disposed; } };
  } catch (error) {
    try { dispose(); } catch {}
    throw error;
  }
}
