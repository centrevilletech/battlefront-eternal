// VehicleTarget's immutable geometry and combat policy. The generated fleet
// slab supplies every changing health, pose, team and seat field in Rust.
import { KIND_VEHICLE_TARGET, F3HullSetupMirror, putF3HullSetup, putF3BodyName } from './sim/gen/index.js';

const HEALTH = new Map([['person',0],['animal',1],['droid',2],['vehicle',3],['building',4]]);

export class F3Hulls {
  constructor(phase, vehicles) {
    this.phase = phase;
    this.vehicles = vehicles;
    this.members = [];
    this.services = phase.factory.context.ordnance.sim.nativeServices;
    this.setup = new F3HullSetupMirror();
    this.record = {words:phase.layout.hullSetupWords, put:putF3HullSetup};
    if (vehicles) {
      // Vehicles.variant adopts a new pooled hull once. Future lives reuse
      // that binding; no health or rig data comes back through this path.
      vehicles.f3 = this;
      vehicles.f2.onPrepared = vehicle => this.prepare(vehicle);
      // The fleet cached its original enabled-byte pointer before F3 joined.
      // Both bridges must now write the reservation shared by native casts.
      vehicles.f2.flags = phase.flags;
      for (let i = 0; i < vehicles.vehicles.length; i++) this.prepare(vehicles.vehicles[i]);
      for (let i = 0; i < vehicles.f2.prepared.length; i++) this.prepare(vehicles.f2.prepared[i]);
    }
  }

  // Vehicle.damage and seat commands can run after JS AI advanced a shared
  // stream. Reuse F1's outer boundary: refresh foreign facts and RNG before
  // entry, then drain notices only after native crew and blast work returns.
  beforeCommand(owner, words, replyWords) {
    const p = this.phase;
    if (p.active) throw Error('fleet command reentered the owned phase');
    p.owners.prepare(owner);
    this.services.beforeStep();
    p.counters.wasmCalls++; p.counters.commandCalls++;
    p.counters.inputFields += words;
    p.counters.outputFields += replyWords;
  }
  afterCommand() { this.services.afterStep(); }

  // Vehicle.blastSphere prefers its model sphere and widens it by the box.
  // Store its local center once; Rust applies the current canonical matrix.
  prepare(vehicle) {
    const p = this.phase, row = this.setup;
    p.items.bindTarget(vehicle.itemTarget,vehicle,true);
    const sphere = this.vehicles.templates.get(vehicle.def.model)?.rig.sphere;
    const radius = 0.5 * vehicle.size.length();
    row.hull = vehicle; row.member = vehicle.f2Prepared ? null : vehicle.aiTarget;
    row.health = HEALTH.get(vehicle.healthType) ?? 0;
    row.category = vehicle.aiTarget.category; row.kind = vehicle.aiTarget.kind;
    row.key = vehicle.f2Prepared ? '' : `v${this.vehicles.vehicles.indexOf(vehicle)}`;
    if (sphere) {
      row.blastCenter.set(sphere[0],sphere[1],sphere[2]);
      row.blastRadius = Math.max(sphere[3],radius);
    } else {
      row.blastCenter.copy(vehicle.center); row.blastRadius = radius;
    }
    const ordnance = p.factory.context.ordnance.sim;
    row.explosion = vehicle.def.explosion ? ordnance.explosionId(vehicle.def.explosion) : null;
    const final = vehicle.def.finalExplosion || vehicle.def.explosion;
    row.finalExplosion = final ? ordnance.explosionId(final) : null;
    p.bridge.callArgs(this.record,row); p.x.f3_hull();
    if (!vehicle.f2Prepared) this.members[vehicle.aiTarget.id] = true;
    // Private variants still have stable kernel and store body handles.
    // An instant first-spawn hit must resolve them without a naming import.
    p.bodyName.body = vehicle.body; p.bodyName.kernel = vehicle.body.raySlot;
    p.bridge.views();
    putF3BodyName(p.bridge,p.bodyName,p.bridge.f,p.layout.input / 8,p.heap);
    p.x.f3_body_names(1);
  }

  // Vehicles.variant's structural append precedes ordinary damage/shot
  // notices. Bind once even if an aimer and the final drain see this birth.
  // All solver state and matrices arrive through the normal fleet publication.
  created(event) {
    const v = event.hull;
    if (!v.f2Prepared) return;
    const p = this.phase, vs = this.vehicles, c = p.collision, units = p.registry.units;
    v.f2Prepared = false;
    if (event.index !== vs.vehicles.length) throw Error('fleet birth order');
    vs.vehicles.push(v); v.slot.variants.set(v.name,v);
    const member = v.aiTarget;
    member.id = event.memberId;
    units.list.push(member); units.byIdMap.set(member.id,member); units.nextId = member.id + 1;
    p.bridge.refs[KIND_VEHICLE_TARGET].slotOf(member);
    this.members[member.id] = true;
    p.registry.prepareFact(member.id);
    c.bodies.push(v.body); c.bodySerial = event.bodySerial;
    v.body.offset = event.bodySerial * 0x1000000;
    c.bodiesVersion++; c.rayOrder = c.bodiesVersion;
    p.bodyVersion = vs.f2.bodiesVersion = c.bodiesVersion;
  }

  // Vehicle.onDamaged's journal reads the sampled life and position. Keeping
  // the normal hook lets presentation observers retain their pooled object.
  notice(event) {
    if (event.event === 'damaged') this.vehicles.onDamaged?.(event.hull,event.amount,event.attacker,event);
    else if (event.event === 'explosionEffect') this.phase.presentEffect('explosion',event.position);
  }
}
