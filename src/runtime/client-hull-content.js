// The page owns replicas, never F2's world clock or canonical fleet phase.
// Resolve immutable hull data once, then construct independent native sessions.
import { VehicleContent } from '../vehicle-content.js';
import { classKeys, giveMapRules, json, pullPose } from '../vehicle-phase-data.js';
import { FLIGHT_CLASSES } from '../flight/config.js';
import { KIND_VEHICLE, KIND_VEHICLES } from '../sim/gen/index.js';
export class ClientHullContent extends VehicleContent {
  constructor(vehicles, bridge, map, pack) {
    super(vehicles, bridge);
    this.classes = new Set(); this.adopted = new WeakSet();
    // Water, map limits and flight tuning are immutable session prerequisites.
    // Loading them here does not enroll the replicas in a world phase.
    giveMapRules(bridge, vehicles.water, map);
    if (!this.pack(pack) && vehicles.vehicles.length) throw Error('prediction requires the hull pack');
    this.x.veh_flight_env(bridge.strings.id(json(vehicles.flightEnvironment ?? {})), vehicles.maxFlyHeight);
    for (const [name, tuning] of Object.entries(FLIGHT_CLASSES)) for (const [key, value] of Object.entries(tuning)) this.retune(name, key, value);
    this.bodiesIn();
  }
  // predictionVehicle used the loaded facade's class and rig. Register them
  // only when first piloted, including a variant born after the battle began.
  // This row is construction data, never the authority for a page frame.
  adopt(v) {
    if (this.adopted.has(v)) return;
    const b = this.bridge;
    if (!this.classes.has(v.name)) {
      this.classes.add(v.name);
      this.x.veh_class(b.strings.id(v.name), b.strings.id(json(classKeys(v.def))), v.turnRate);
    }
    // veh_pull resolves the rig through the resource store, including late
    // variants. These cold references are never refreshed by the frame path.
    b.writeSome(KIND_VEHICLE, [v]);
    b.write(KIND_VEHICLES, [this.vehicles]);
    const pool = b.refs[KIND_VEHICLE];
    v.simSlot = pool.slotOf(v); v.simGen = pool.gens[v.simSlot];
    const pose = pullPose(v), at = this.input(pose.length);
    b.f.set(pose, at);
    if (!this.x.veh_pull(v.simSlot, v.aiTarget.generation, pose.length, 0)) throw Error('prediction could not retain the hull rig');
    this.adopted.add(v);
  }
}
