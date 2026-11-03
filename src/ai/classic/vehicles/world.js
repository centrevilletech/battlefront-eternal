import { selectWeapon, switchWeapon } from "../weapons-ai.js";
import { pilotInput } from "../../../flight/ai.js";
import { captureRegion, cpCenter } from "../patrol.js";

// What the vehicle AI asks of other packages' module functions, and the
// reads whose answer is worked out when asked (a unit's hit shapes, a
// turret's rig), each as a method of this one object: the calls a capture
// of the vehicle AI records (tools/sim-capture/vehicle-ai.mjs) and the Rust
// port's boundary trait names (rust/sim/src/ai/vehicles/world.rs). The rest
// of what it asks of the battle is already a method of a live object
// (collision.cast, units.query, a seat's origin, a helper's goto). Each
// method calls straight through: the vehicle AI behaves as it did before.
export class VehicleAIWorld {
  // The weapon choice and switch (src/ai/classic/weapons-ai.js).
  selectWeapon(controller, unit, target, switchAllowed, world, out) {
    return selectWeapon(controller, unit, target, switchAllowed, world, out);
  }

  switchWeapon(controller, unit, index) {
    return switchWeapon(controller, unit, index);
  }

  // A bot pilot's stick (src/flight/ai.js).
  pilotInput(unit, request, aim, dt, env, out) {
    return pilotInput(unit, request, aim, dt, env, out);
  }

  // A post's capture region and centre as the AI reads them (patrol.js).
  cpCenter(ai, post, out) {
    return cpCenter(ai, post, out);
  }

  captureRegion(ai, post) {
    return captureRegion(ai, post);
  }

  // A unit's bounding sphere this tick (a soldier's is worked out on the
  // first ask of each tick; a hull's or a turret's from its pose).
  sphere(x) {
    return x.sphere;
  }

  // A turret's gun pivot in the world: its gun's (or fire point's) world
  // place with the rig's matrices brought up to date, else the stand-in
  // offset from the turret's matrix.
  turretPivot(turret, out) {
    const node = turret.gun || turret.firePoint;
    if (node) {
      turret.rig?.root?.updateMatrixWorld?.(true);
      return node.getWorldPosition(out);
    }
    return out.set(0, 1.65, 0.32).applyMatrix4(turret.matrix);
  }
}

// One for every battle the process runs: it keeps no state.
export const port = new VehicleAIWorld();
