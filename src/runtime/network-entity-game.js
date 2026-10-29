import * as THREE from 'three';
import {presentEntityEvent} from './entity-presentation-events.js';
import {applyUnitState,UNIT_STATE_SCHEMA} from './entity-unit-state.js';
import {applyWorldPresentation} from './entity-world-state.js';
import {decodeMountedControl} from './network-mounted-control.js';
import {EntityRemotePresentation} from './network-entity-remotes.js';
import { EntityItemPresentation } from './network-entity-items.js';
import { CombatPresentation } from './network-entity-combat.js';
import { createCombatRegistry } from './network-combat-registry.js';
import { presentFeedback } from './network-combat-feedback.js';
import { emitSound } from '../audio/events.js';
import { SnapshotClient } from './snapshot-client.js';
import { SnapshotTimeline, SnapshotPresentation } from './snapshot-timeline.js';
import { applyOwnPlayer, decodeOwnPlayer } from './network-entity-player.js';
import { ENTITY_POSTURES, ENTITY_SOLDIER_STATES, ENTITY_VEHICLE_STATES } from './network-entity-capture.js';
import { assignUnitId } from './entity-identity.js';
import { applyWorldSummary, decodeWorldSummary } from './network-entity-summary.js';
import { unseatBody, poseTurret } from '../seats.js';
import { applyChargeStep } from '../weapons.js';
import { goneUp } from '../droideka.js';
import { followFlightState } from '../flight/gear.js';

// Same off-field presentation cleanup as Unit.remove, without returning the
// detached ownership proxy to the gameplay factory pool a second time.
function hideOwnershipProxy(unit) {
  unit.alive = false; unit.removed = true; unit.actor.root.visible = false;
  if (unit.character?.unit === unit) unit.character.unit = null;
  unit.character = null;
  unit.jetFlame?.stop(); unit.jetFlame = null;
  unit.combat?.clearCharge?.();
}

// The shared gameplay adapter for local and remote servers. Content builds
// models; independently versioned records establish all live world state.
//
// Mounts: a hull is the shared Vehicle the simulation drives; a seat lease is
// the shared SeatLease the SeatReader installs; a turret is the shared Turret
// building. Nothing here duplicates boarding, exit, destruction or aim rules;
// it installs what the server decided and lets the existing presentation
// (Vehicle.present/poseCrew, TurretSeats.pose, poseTurret) draw it.
export class EntityGameState {
  constructor(game, registry, { beforeApply = () => {}, onCorrection = () => {}, onApplied = () => {} } = {}) {
    Object.assign(this, { game, registry, beforeApply, onCorrection, onApplied });
    this.receiver = new SnapshotClient();
    this.timeline = new SnapshotTimeline(); this.presentation = new SnapshotPresentation(this.timeline);
    const combatRegistry=registry.combat ?? createCombatRegistry(game.data);
    this.combat=new CombatPresentation({ordnance:game.ordnance,definition:id=>combatRegistry.definitions.get(id),predictedShots:game.predictedShots,onFeedback:event=>presentFeedback(game,event),
      muzzle:(event,def)=>{const unit=game.units.byId(event.ownerId);if(!unit||unit.generation!==event.ownerGeneration||unit.removed||unit.seat||!unit.combat?.arsenal)return null;const weapon=unit.combat.arsenal.weapons.find(weapon=>weapon.def?.ordnance===def);if(!weapon)return null;const held=unit.combat.firePoint(weapon)?.position?.clone()??null;if(held)return held;const out=new THREE.Vector3();return unit.soldier?.nextVisualGun?.(out)?out:null;},
      onPresentation:event=>presentEntityEvent(game,event,registry),onExplosion:(def,position)=>game.shakeExplosion?.(def,position)});
    this.items = game.items ? new EntityItemPresentation(game.items,combatRegistry) : null;
    this.remotes = game.remotes ? new EntityRemotePresentation(game.remotes,combatRegistry) : null;
    this.ready = false; this.previous = null; this.pendingCorrection = null; this.pendingEligibility = null;
    // Slots named by mount records, so a hull the client has never built can be
    // made from its vehicle record; seats whose mount has not arrived yet.
    this.slots = new Map(); this.pendingSeats = new Map(); this.pendingState = new Map(); this.deferred = 0; this.presentedTick = null;
  }
  initialize() {
    const g=this.game;
    // Content constructs the scene; state is supplied solely by the host.
    for(const unit of g.units.all())if(unit.soldier){unit.alive=false;unit.removed=true;unit.actor.root.visible=false;}
    for(const vehicle of g.vehicles?.vehicles || [])this.destroyHull(vehicle);
    this.baseline();
  }
  baseline() { this.slots.clear(); this.pendingSeats.clear(); this.pendingCorrection = null; this.pendingEligibility = null; this.resetOwnershipProxy(); this.game.ordnance.clearProjectilePresentation(); this.game.items?.clearPresentation(); this.remotes?.reset(null); this.ready = true; }
  resetOwnershipProxy() {
    const proxy = this.ownProxy; if (!proxy) return;
    hideOwnershipProxy(proxy);
    proxy.actor.root.removeFromParent();
    const built = this.game.factory.built, index = built?.indexOf(proxy) ?? -1;
    if (index >= 0) built.splice(index, 1);
    this.ownProxy = null;
  }
  receiveGameplay(records, tick) {
    if (!this.ready) throw Error('Entity state arrived before world baseline application');
    const packet = this.receiver.gameplayRecords(records, tick);
    this.beforeApply();
    for (const record of packet.records) if (record.kind === 'combat') this.combat.receive(record, { epoch: packet.epoch, tick: packet.tick });
    this.onApplied(packet);
  }
  receive(bytes) {
    if (!this.ready) throw Error('Entity state arrived before world baseline application');
    const packet = this.receiver.receive(bytes); if (!packet) return;
    this.beforeApply();
    if (packet.epochChanged) { this.timeline.reset(packet.epoch); this.presentation.reset(); this.items?.reset(packet.epoch); this.remotes?.reset(packet.epoch); this.previous = null; this.pendingCorrection = null; this.pendingEligibility = null; this.slots.clear(); this.pendingSeats.clear(); }
    const corrected = packet.records.some(record => record.kind === 'correction');
    for (const record of packet.records) if (record.kind === 'correction') this.prepareOwn(record);
    for (const record of packet.records) if (record.kind === 'mount' && record.slot !== null) this.slots.set(record.id, { index: record.slot, generation: record.generation });
    for (const record of packet.records) if (record.kind === 'vehicle') this.vehicle(record);
    // An unordered vehicle packet may have arrived before its slot metadata.
    // Reuse the receiver's bounded latest state once that prerequisite exists;
    // a stored tombstone or another generation must never create a stale hull.
    for (const record of packet.records) if (record.kind === 'mount' && record.slot !== null) {
      const latest = this.receiver.get('vehicle', record.id);
      if (latest?.kind === 'vehicle' && latest.generation === record.generation &&
          !packet.records.some(value => value.kind === 'vehicle' && value.id === record.id)) this.vehicle(latest);
    }
    for (const record of packet.records) if (record.kind === 'turret') this.turret(record);
    for (const record of packet.records) if (record.kind === 'droideka') this.droideka(record);
    for (const record of packet.records) if (record.kind === 'soldier') this.soldier(record, corrected);
    for (const record of packet.records) if (record.kind === 'tombstone') this.remove(record);
    for (const change of packet.seatChanges) this.seat(change);
    for (const [id, change] of [...this.pendingSeats]) { this.pendingSeats.delete(id); this.seat(change); }
    for (const record of packet.records) if (record.kind === 'mount') this.mount(record);
    for (const record of packet.records) if (record.kind === 'summary') {
      if (record.schema >= 6) { this.pendingState.set(record.id, record); continue; }
      if (record.schema === 3) {
        const summary = decodeWorldSummary(record, this.registry);
        this.pendingEligibility = { record, summary };
      } else applyWorldSummary(this.game, record, { registry: this.registry });
    }
    for (const record of packet.records) if (record.kind === 'correction') {
      if (!this.pendingCorrection || record.tick >= this.pendingCorrection.record.tick) this.pendingCorrection = { record, packet };
    }
    this.retryCorrection(packet.tick);
    this.applyEligibility();
    for(const [id,record]of this.pendingState){
      const applied=record.schema===UNIT_STATE_SCHEMA?applyUnitState(this.game,record,this.registry):applyWorldPresentation(this.game,record);
      if(applied||packet.tick-record.tick>120)this.pendingState.delete(id);
    }
    for(const record of packet.records)if(record.kind==='remote'){if(!this.remotes)throw Error('missing remote presentation');this.remotes.receive(record,packet.ack.epoch);}
    for(const record of packet.records)if(record.kind==='item'){if(!this.items)throw Error('missing item presentation');this.items.receive(record,packet.ack.epoch);}
    for(const record of packet.records)if(record.kind==='combat')this.combat.receive(record,{epoch:packet.ack.epoch,tick:packet.tick});
    if (packet.sample) this.timeline.addSnapshot(packet.sample.tick, packet.sample.poses, packet.sample.epoch);
    this.onApplied(packet);
  }
  applyEligibility() {
    const pending = this.pendingEligibility, own = this.game.playerUnit;
    if (!pending || pending.summary.playerId !== own.id || pending.summary.generation !== own.generation) return;
    const summary = applyWorldSummary(this.game, pending.record, { registry: this.registry });
    this.eligibility = summary;
    this.pendingEligibility = null;
  }
  prepareOwn(record) {
    const g = this.game, unit = g.playerUnit, data = decodeOwnPlayer(record), className = this.registry.classes.get(data.classId);
    if (!className || unit.className !== className && !g.bodies.get(className)) throw Error('Missing own class prerequisite');
    if (record.id !== unit.id) {
      const oldId = unit.id, oldGeneration = unit.generation;
      let other = g.units.byId(record.id);
      // The persistent local Unit adopts bodies, but server IDs retain their
      // original class. Never put the incoming class under the previous ID.
      for (const body of [unit, other]) if (body?.seat?.vehicle) {
        const old = body.seat, slot = old.vehicle.seats[old.index];
        g.client.seats.setLease(null, old.vehicle, null, value => { slot.lease = value; }, old.index);
        if (body === unit) g.client.seats.setDriver(-1);
      }
      this.presentation.unbind(oldId); this.presentation.unbind(record.id);
      this.timeline.remove(oldId); this.timeline.remove(record.id);
      if (other && other.className !== unit.className) {
        g.units.remove(other);
        hideOwnershipProxy(other);
        const spare = this.ownProxy;
        this.ownProxy = other;
        other = spare;
        if (other) g.units.add(other);
      } else if (!other && this.ownProxy) {
        other = this.ownProxy; this.ownProxy = null; g.units.add(other);
      }
      if (!other) other = g.factory.build(unit.className);
      if (other.className !== unit.className) throw Error('Invalid own identity proxy class');
      g.units.byIdMap.delete(other.id); g.units.byIdMap.delete(oldId);
      unit.id = record.id; other.id = oldId; other.generation = oldGeneration;
      g.units.nextId = Math.max(g.units.nextId, record.id + 1);
      hideOwnershipProxy(other);
      other.presentation.reset();
      g.units.byIdMap.set(unit.id, unit); g.units.byIdMap.set(other.id, other);
      this.previous = null;
    }
    if (unit.generation !== record.generation && unit.seat) this.clearSeat(unit);
    if (unit.className !== className) { unit.combat.dead = true; g.adopt(g.bodies.get(className)); }
    unit.generation = record.generation;
  }
  soldier(record, corrected = true) {
    const g = this.game, className = this.registry.classes.get(record.classId);
    if (!className || !ENTITY_POSTURES[record.posture] || !ENTITY_SOLDIER_STATES[record.state]) throw Error('Unsupported entity soldier definition');
    // The weapon in his hands charging (the bowcaster held), shown for him
    // and for everyone else alike (PlayerCombat.presentCharge).
    const charged = (unit) => { const name = record.weapon ? this.registry.weapons.get(record.weapon) : null; const arsenal = unit.combat.arsenal; applyChargeStep(arsenal, name ? arsenal.weapons.find(w => w.name === name) ?? null : null, record.charge ?? 0); };
    if (record.id === g.playerUnit.id) {
      charged(g.playerUnit);
      // Exact own correction is the sole physics authority. Without one (a
      // seat with no replay schema yet) the life fields still arrive here;
      // the seat and hull place the body.
      if (corrected) return;
      const own = g.playerUnit, state = ENTITY_SOLDIER_STATES[record.state], wasDead = own.combat.dead;
      own.generation = record.generation; own.team = record.team;
      own.dead = own.combat.dead = own.soldier.dead = state === 'dead' || state === 'thrownDead'; own.alive = !own.dead; own.combat.health = record.health;
      own.combat.updateHeld?.();
      // The entities datagram lands before the correction (snapshot-host
      // order), so this branch is where the page body first dies: start
      // the death here, or correct() later finds combat.dead already true
      // and never does, leaving the corpse standing in idle.
      if (own.dead && !own.removed && !wasDead) { own.animator.die?.(); g.presentEvent?.('died', own); }
      return;
    }
    let unit = g.units.byId(record.id);
    if (!unit) { unit = g.factory.build(className); assignUnitId(g.units, unit, record.id); }
    if (unit.className !== className || unit.walker) throw Error('Unsupported entity soldier body');
    const wasDead=unit.dead, wasHealth=unit.combat.health;
    const newLife = unit.generation !== record.generation, s = unit.soldier, state = ENTITY_SOLDIER_STATES[record.state];
    if (newLife && unit.seat) this.clearSeat(unit);
    unit.generation = record.generation; unit.team = unit.combat.team = record.team; unit.removed = false;
    unit.dead = unit.combat.dead = s.dead = state === 'dead' || state === 'thrownDead'; unit.alive = !unit.dead;
    unit.combat.health = record.health; s.position.fromArray(record.position); s.velocity.fromArray(record.velocity);
    s.aimYaw = record.yaw; s.aimPitch = record.pitch; s.posture = ENTITY_POSTURES[record.posture];
    // A body still flying after a blast killed it keeps the thrown pose.
    s.air = state === 'thrownDead' ? 'thrown' : ['jump','fall','thrown','jet'].includes(state) ? state : null;
    s.dive = state === 'diveRight' ? 1 : state === 'diveLeft' ? -1 : 0;
    if (record.weapon) {
      const name = this.registry.weapons.get(record.weapon), weapon = unit.combat.arsenal.weapons.find(w => w.name === name);
      if (!weapon) throw Error('Missing remote weapon prerequisite');
      if (unit.combat.arsenal.current(0) !== weapon) unit.combat.arsenal.select(weapon);
    }
    charged(unit);
    unit.tickCount++;
    if (newLife) { unit.presentation.reset(); unit.animator.reset(s.aimYaw); unit.combat.blowApart = false; }
    // A droid blown apart stays gone for this life (Unit.goUp), whatever the
    // later records of its lingering body say.
    if (unit.dead && unit.combat.blowApart) unit.goUp?.(); else unit.actor.root.visible = true;
    if(unit.dead&&(newLife||!wasDead)){unit.presentation.deathPending=true;emitSound('actorDeath',unit.combat,0);g.presentEvent?.('died',unit);}
    if(!unit.dead&&!newLife&&record.health<wasHealth)emitSound('actorHurt',unit.combat,wasHealth-record.health);
    unit.combat.updateHeld?.();
    // A seated body rides its mount (Vehicles.presentRiders, TurretSeats.pose);
    // its own timeline would drag it behind the hull between samples.
    if (unit.seat) this.presentation.unbind(unit.id);
    else this.presentation.bindSoldier(unit.id, unit.generation, unit.presentation);
  }
  droideka(record) {
    const g = this.game, className = this.registry.classes.get(record.classId);
    if (!className) throw Error('Unsupported Droideka definition');
    // Own physics and life changes come exclusively from exact corrections.
    if (record.id === g.playerUnit.id) return;
    let unit = g.units.byId(record.id);
    if (!unit) { unit = g.factory.build(className); assignUnitId(g.units, unit, record.id); }
    if (!unit.walker || unit.className !== className) throw Error('Unsupported Droideka body');
    const d = unit.soldier, wasDead = unit.dead, wasHealth = unit.combat.health, newLife = unit.generation !== record.generation;
    if ((!d.shield && (record.shield || record.shieldOn)) || d.shield && record.shield > d.shield.max) throw Error('Droideka shield prerequisite mismatch');
    const weaponName = record.weapon ? this.registry.weapons.get(record.weapon) : null;
    const weapon = weaponName ? unit.combat.arsenal.weapons.find(value => value.name === weaponName) : null;
    if (record.weapon && !weapon) throw Error('Missing remote Droideka weapon prerequisite');
    if (newLife && unit.seat) this.clearSeat(unit);
    unit.generation = record.generation; unit.team = unit.combat.team = record.team; unit.removed = false;
    unit.dead = unit.combat.dead = d.dead = record.dead; unit.alive = !record.dead;
    unit.combat.health = record.health; d.position.fromArray(record.position); d.velocity.fromArray(record.velocity);
    d.aimYaw = record.yaw; d.aimPitch = record.pitch;
    for (const key of ['mode','time','reverse','roll','lean','legs']) d[key] = record[key];
    d.previous.copy(d.position); d.settled.copy(d.position);
    if (d.shield) { d.shield.value = record.shield; d.shield.on = record.shieldOn; }
    if (weapon && unit.combat.arsenal.current(0) !== weapon) unit.combat.arsenal.select(weapon);
    unit.tickCount++;
    if (newLife) { unit.presentation.reset(); unit.animator.reset(d.aimYaw); }
    // Gone up (its clip played out, its final blast and pieces gone off):
    // the body dissolves and stays gone for this life (Unit.goUp), whatever
    // the later records and summaries of its lingering life say.
    if (goneUp(record)) unit.goUp?.(); else unit.actor.root.visible = true;
    unit.combat.updateHeld?.();
    if (unit.dead && (newLife || !wasDead)) { emitSound('actorDeath', unit.combat, 0); g.presentEvent?.('died', unit); }
    if (!unit.dead && !newLife && record.health < wasHealth) emitSound('actorHurt', unit.combat, wasHealth - record.health);
    this.presentation.bind(unit.id, unit.generation, 'droideka', unit.presentation);
  }
  // The hull for a vehicle record: the one the registry knows, else one made
  // from the slot a mount record named (Vehicles.variant, as the legacy
  // SeatReader did from `created`). Null when nothing can be built yet.
  hull(record) {
    const g = this.game, system = g.vehicles; if (!system) return null;
    let vehicle = system.vehicles.find(v => v.aiTarget.id === record.id);
    if (vehicle || record.kind !== 'vehicle') return vehicle || null;
    const className = this.registry.classes.get(record.classId), metadata = this.slots.get(record.id);
    const slot = metadata?.generation === record.generation ? system.slots?.[metadata.index] : null;
    if (!className || !slot) { this.deferred++; return null; }
    vehicle = system.variant(slot, className);
    assignUnitId(g.units, vehicle.aiTarget, record.id);
    return vehicle;
  }
  vehicle(record) {
    const vehicle = this.hull(record); if (!vehicle) return;
    if (this.registry.classes.get(record.classId) !== vehicle.name) throw Error('Missing semantic vehicle lifecycle prerequisite');
    const state = ENTITY_VEHICLE_STATES[record.state]; if (!state) throw Error('Unsupported entity vehicle state');
    if (state === 'Dying') { this.dyingHull(vehicle, record); return; }
    const destroyed = record.health === 0 || state === 'Destroyed';
    if (destroyed) { if (vehicle.alive) this.destroyHull(vehicle); vehicle.aiTarget.generation = record.generation; return; }
    // A new life of the hull: the shared spawn resets seats, weapons and
    // state exactly as the server's did, before the record places it.
    const newLife = !vehicle.alive || vehicle.aiTarget.generation !== record.generation;
    if (newLife) { vehicle.spawn(record.team); vehicle.aiTarget.generation = record.generation; }
    // A remote-only packet has no acknowledgement for this pilot's baseline.
    // It may update lifecycle above, but cannot replace exact replay motion.
    // His craft's gear is its own (corrected and replayed); every other
    // craft's follows the flight state its records carry (src/flight/gear.js).
    if (!newLife && this.ownsMountAim(vehicle, 0)) { if (vehicle.flying) followFlightState(vehicle, false); return; }
    vehicle.team = record.team; vehicle.health = record.health;
    vehicle.position.fromArray(record.position); vehicle.velocity.fromArray(record.velocity); vehicle.quaternion.fromArray(record.quaternion);
    if (vehicle.flying) { vehicle.flightState = state; followFlightState(vehicle, true); }
    else {
      const q = vehicle.quaternion;
      vehicle.yaw = Math.atan2(2 * (q.x * q.z + q.w * q.y), 1 - 2 * (q.y * q.y + q.z * q.z));
      vehicle.previousYaw = vehicle.yaw;
    }
    vehicle.previous.copy(vehicle.position);
    // Semantic hull creation skips the simulation/baseline tick capture. Seed
    // shared presentation history before a pilot correction can restore it.
    if (newLife || !vehicle.tickFrom) vehicle.captureTick();
    vehicle.power = record.power; vehicle.root.visible = vehicle.alive;
    vehicle.syncBody(); this.presentation.bindVehicle(record.id, record.generation, vehicle);
    // A mount can be acknowledged before its hull exists. Reuse its retained
    // exact-life pose when the hull arrives; unchanged heartbeats may be deduped.
    const mount = this.receiver.get('mount', record.id);
    if (mount?.generation === record.generation) this.mount(mount);
  }
  // The crew pose beside the hull: lean, walk cycle and every seat's aim, for
  // Vehicle.present/poseCrew and the riders. Own seats included: the driver's
  // preview adds the look the server has not seen yet on top.
  mount(record) {
    const vehicle = this.game.vehicles?.vehicles.find(v => v.aiTarget.id === record.id);
    if (!vehicle || !vehicle.alive || vehicle.aiTarget.generation !== record.generation) return;
    if (!this.ownsMountAim(vehicle, 0)) {
      vehicle.tilt = vehicle.previousTilt = record.tilt; vehicle.roll = vehicle.previousRoll = record.roll;
    }
    vehicle.walkPhase = record.walkPhase; vehicle.walkBlend = record.walkBlend;
    for (let i = 0; i < vehicle.seats.length && i < record.seats.length; i++) {
      const seat = vehicle.seats[i];
      if (this.ownsMountAim(vehicle, i)) continue;
      seat.aimYaw = record.seats[i].yaw; seat.pitch = record.seats[i].pitch;
      if (seat.lastYaw === null) seat.lastYaw = seat.aimYaw;
    }
  }
  // A stationary turret: its registry identity, build state and the angles the
  // tick left. The drum and gun are posed by the shared Turret.present and
  // poseTurret; a manning body by TurretSeats.pose.
  turret(record) {
    const g = this.game, turret = g.live?.buildings?.[record.building];
    if (!turret || !turret.rig) { this.deferred++; return; }
    turret.aiTarget ||= g.turretSeats?.attachUnits ? (g.turretSeats.attachUnits(g.units), turret.aiTarget) : null;
    if (!turret.aiTarget) { this.deferred++; return; }
    if (turret.aiTarget.id !== record.id) assignUnitId(g.units, turret.aiTarget, record.id);
    turret.aiTarget.generation = record.generation;
    if (turret.state !== record.state) {
      // Build and destruction change collision and looks through the shared
      // Building.show; no local explosion or damage follows from a record.
      if (record.state === 'built' && turret.lease) this.clearTurretLease(turret);
      if (record.state !== 'built' && turret.lease) this.clearTurretLease(turret);
      turret.show(record.state);
    }
    turret.health = record.health; turret.team = record.team;
    if (!this.ownsMountAim(turret, 0)) {
      if (turret.aimFrom) turret.aimFrom(record);
      else { turret.yaw = record.yaw; turret.pitch = record.pitch; turret.gunYaw = record.gunYaw; turret.gunPitch = record.gunPitch; }
    }
    if (turret.alive) turret.present();
  }
  ownsMountAim(mount, index) {
    const own = this.game.playerUnit, lease = own.seat, previous = this.previous;
    if (!previous || previous.id !== own.id || previous.generation !== own.generation ||
        !lease || lease.released || (lease.vehicle || lease.turret) !== mount || (lease.index || 0) !== index) return false;
    const id = mount.aiTarget?.id, generation = mount.aiTarget?.generation;
    return lease.vehicle && index === 0 && previous.vehicleId === id && previous.vehicleGeneration === generation ||
      previous.mountId === id && previous.mountGeneration === generation && previous.mountSeat === index;
  }
  remove(record) {
    const g = this.game, unit = g.units.byId(record.id); this.presentation.unbind(record.id);
    if (!unit || unit.generation !== record.generation) return;
    if (unit.vehicle) { if (unit.vehicle.alive || unit.vehicle.dying > 0) this.destroyHull(unit.vehicle); }
    else if (unit !== g.playerUnit) { if (unit.seat) this.clearSeat(unit); unit.alive = false; unit.removed = true; unit.actor.root.visible = false; unit.combat?.clearCharge?.(); }
  }
  // A hull the server destroyed: dead, hidden, no collision, every lease let
  // go at once (Vehicle.setHealth's lease loop), the own driver released. The
  // explosion, the crew's deaths and the credit are the server's and arrive
  // as their own records; nothing here deals damage.
  destroyHull(vehicle) {
    const g = this.game;
    vehicle.alive = false; vehicle.health = 0; vehicle.dying = 0;
    if (vehicle.body) vehicle.body.enabled = false;
    vehicle.root.visible = false;
    if (vehicle.flying) vehicle.flightState = 'Destroyed';
    const driver = g.driver;
    if (driver?.vehicle === vehicle) driver.release();
    for (const seat of vehicle.seats) {
      const lease = seat.lease; if (!lease) continue;
      lease.released = true; seat.lease = null;
      if (lease.unit?.seat === lease) { lease.unit.seat = null; unseatBody(lease.unit); }
    }
    vehicle.driver = null; vehicle.humanDriver = null;
    vehicle.velocity.set(0, 0, 0);
    this.presentation.unbind(vehicle.aiTarget.id);
  }
  // A walker the server destroyed that is still playing out its death
  // animation (Vehicle.dyingTick): dead and let go of as any destroyed hull,
  // but drawn where it fell, posed by the time it has left (the record's
  // power, in ms; Vehicle.present). Its blasts are the server's and arrive
  // as their own records; its tombstone, once it is gone, hides it.
  dyingHull(vehicle, record) {
    if (vehicle.alive) this.destroyHull(vehicle);
    vehicle.aiTarget.generation = record.generation;
    vehicle.position.fromArray(record.position); vehicle.quaternion.fromArray(record.quaternion);
    if (!vehicle.flying) {
      const q = vehicle.quaternion;
      vehicle.yaw = vehicle.previousYaw = Math.atan2(2 * (q.x * q.z + q.w * q.y), 1 - 2 * (q.y * q.y + q.z * q.z));
    }
    vehicle.previous.copy(vehicle.position);
    vehicle.dying = record.power / 1000;
    vehicle.root.visible = vehicle.dying > 0;
  }
  // The unit's current lease let go, whichever mount holds it.
  clearSeat(unit) {
    const g = this.game, lease = unit.seat; if (!lease) return;
    const reader = g.client.seats;
    if (lease.turret) {
      const turret = lease.turret;
      if (unit === g.playerUnit && g.gunner?.turret === turret) g.gunner.exit(unit.soldier);
      else reader.setLease(turret, null, null, value => { turret.lease = value; });
      if (turret.lease === lease) turret.lease = null;
      if (turret.gunner === unit.soldier) turret.gunner = null;
    } else if (lease.vehicle) {
      const vehicle = lease.vehicle, slot = vehicle.seats[lease.index];
      reader.setLease(null, vehicle, null, value => { if (slot) slot.lease = value; }, lease.index);
      if (unit === g.playerUnit) reader.setDriver(-1);
    }
    if (unit.seat === lease) { unit.seat = null; unseatBody(unit); }
  }
  clearTurretLease(turret) {
    const lease = turret.lease; if (!lease) return;
    if (lease.unit) this.clearSeat(lease.unit);
    else { lease.released = true; turret.lease = null; turret.gunner = null; }
  }
  seat({ id, generation, seat }) {
    const g = this.game, unit = g.units.byId(id); if (!unit || unit.generation !== generation) return;
    const reader = g.client.seats;
    const current = unit.seat, currentMount = current ? (current.vehicle || current.turret)?.aiTarget : null;
    const same = seat && current && currentMount?.id === seat.vehicleId && currentMount.generation === seat.vehicleGeneration && current.index === seat.seatIndex && !current.released;
    if (same) return;
    if (current) this.clearSeat(unit);
    if (!seat) return;
    const own = unit === g.playerUnit;
    const vehicle = g.vehicles?.vehicles.find(v => v.aiTarget.id === seat.vehicleId && v.aiTarget.generation === seat.vehicleGeneration);
    if (vehicle) {
      if (!vehicle.alive || !vehicle.seats[seat.seatIndex]) { this.pendingSeats.set(id, { id, generation, seat }); return; }
      const slot = vehicle.seats[seat.seatIndex];
      if (slot.lease && slot.lease.unit !== unit) { const other = slot.lease.unit; if (other) this.clearSeat(other); else { slot.lease.released = true; slot.lease = null; } }
      reader.setLease(null, vehicle, [id, own ? 1 : 0, seat.seatIndex === 0 ? 'pilot' : 'gunner', generation], value => { slot.lease = value; }, seat.seatIndex);
      unit.targetable = false; if (unit.combat) unit.combat.mounted = true;
      if (own) { vehicle.seatIndex = seat.seatIndex; reader.setDriver(g.vehicles.vehicles.indexOf(vehicle)); g.driver?.sync?.(); }
      else g.vehicles.syncRider?.(slot.lease);
      return;
    }
    const turret = (g.turretSeats?.turrets || []).find(t => t.aiTarget?.id === seat.vehicleId && t.aiTarget.generation === seat.vehicleGeneration);
    if (!turret) { this.pendingSeats.set(id, { id, generation, seat }); return; }
    if (!turret.alive) return;
    if (turret.lease && turret.lease.unit !== unit) this.clearTurretLease(turret);
    if (own && g.gunner) {
      // The shared Gunner adapter mans it (lease, mounted combat, views); the
      // turret record then restores the server's drum and gun angles.
      const yaw = turret.yaw, pitch = turret.pitch;
      g.gunner.enter(turret, unit.soldier); if (g.combat) g.combat.mounted = true;
      turret.yaw = yaw; turret.pitch = pitch;
      g.client.seats.setGunner?.(g.live.buildings.indexOf(turret));
    } else {
      reader.setLease(turret, null, [id, 0, 'gunner', generation], value => { turret.lease = value; });
      if (turret.lease) { turret.lease.generation = generation; g.turretSeats.pose?.(turret.lease, 0); }
    }
    this.presentation.unbind(unit.id);
  }
  retryCorrection(tick) {
    const pending = this.pendingCorrection; if (!pending) return;
    const { record, packet } = pending, unit = this.game.playerUnit;
    // The queue holds one original packet, including its own input ACK. A
    // later lifecycle packet must never lend its ACK to an older correction.
    if (tick - record.tick > 120 || record.id !== unit.id || record.generation !== unit.generation ||
        this.previous?.epoch === packet.ack.epoch && this.previous.tick > record.tick) {
      this.pendingCorrection = null; return;
    }
    if (this.correct(record, packet)) this.pendingCorrection = null;
  }
  correct(record, packet) {
    const g = this.game, unit = g.playerUnit, data = decodeOwnPlayer(record), wasDead = unit.combat.dead;
    if (record.id !== unit.id) throw Error('Entity correction does not own this player');
    if (data.epoch !== packet.ack.epoch || data.contentEpoch !== this.registry.contentEpoch ||
        this.registry.classes.get(data.classId) !== unit.className) throw Error('Invalid own correction content prerequisite');
    const vehicle = data.vehicleId ? g.vehicles.vehicles.find(v => v.aiTarget.id === data.vehicleId) : null;
    // Unordered own packets may precede the hull/lease. Only these lifecycle
    // prerequisites are deferrable; malformed state still fails validation.
    if (data.vehicleId) {
      if (vehicle?.aiTarget.generation > data.vehicleGeneration) return true;
      if (!vehicle || vehicle.aiTarget.generation !== data.vehicleGeneration || !vehicle.alive ||
          unit.seat?.vehicle !== vehicle || unit.seat.index !== 0) return false;
    } else if (data.mounted) {
      const mounted=decodeMountedControl(data.mounted);
      const object=mounted.mode===2 ? g.vehicles?.vehicles.find(value=>value.aiTarget.id===mounted.id)
        : g.turretSeats?.turrets.find(value=>value.aiTarget?.id===mounted.id);
      if(object?.aiTarget.generation>mounted.generation)return true;
      if(!object||object.aiTarget.generation!==mounted.generation||!object.alive)return false;
      if(mounted.mode===2 ? unit.seat?.vehicle!==object||unit.seat.index!==mounted.index
        : unit.seat?.turret!==object)return false;
    } else if (unit.seat) return false;
    g.validateAuthoritativeHumanTeam?.(data.team);
    unit.presentation.beforeStep();
    const context = applyOwnPlayer({ unit, vehicle, vehicleId: data.vehicleId, vehicleGeneration: data.vehicleGeneration,
      epoch: packet.ack.epoch, registry: this.registry, record, previous: this.previous,
      remoteLifecycle: data.remote ? this.receiver.get('remote',data.remote.id) : null });
    if (context.team !== g.playerTeam) g.applyAuthoritativeHumanTeam(context.team);
    unit.dead = unit.soldier.dead = unit.combat.dead; unit.alive = !unit.dead && !unit.removed;
    unit.actor.root.visible = !unit.removed; unit.tickCount++; unit.combat.updateHeld?.(); unit.presentation.afterStep();
    if (context.reset) { unit.presentation.reset(); unit.animator.reset(unit.soldier.aimYaw); }
    if(unit.dead&&!unit.removed&&(!wasDead||context.reset)){unit.animator.die?.();g.presentEvent?.('died',unit);}
    this.previous = context; this.onCorrection(context, packet); return true;
  }
  present(tick) {
    this.items?.present(tick); this.remotes?.present(tick);
    const g = this.game, dt = this.presentedTick === null ? 0 : Math.max(0, Math.min(.25, (tick - this.presentedTick) / 60));
    this.presentedTick = tick;
    this.presentation.update(tick, { playerId: g.playerUnit.id, vehicleId: g.driver?.active ? g.driver.vehicle.aiTarget.id : null });
    // Vehicle.present reads the frame's dt from the pose to turn the aimers.
    for (const vehicle of g.vehicles?.vehicles || []) if (vehicle.networkPose) vehicle.networkPose.dt = dt;
    // Manned turrets: the drum and gun at the replicated angles, the body in the
    // seat (TurretSeats.pose). The own gunner's are the Gunner's to pose.
    for (const turret of g.turretSeats?.turrets || []) {
      const lease = turret.lease; if (!lease || lease.human || !turret.alive) continue;
      turret.settleTurn?.(tick);
      poseTurret(turret, turret.yaw);
      if (lease.unit?.soldier) g.turretSeats.pose(lease, 0);
    }
  }
}
