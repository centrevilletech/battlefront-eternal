import {RecordMap,PacketLease,retainRecord,releaseRecord} from './network-records.js';
import {NetKind as NK,NetLimits as NL,NetRecord as NR} from './network-layout.js';
import * as THREE from 'three';
import {NetworkMounts} from './network-mounts.js';
import {NetworkPreview} from './network-preview.js';
import {presentEntityEvent} from './entity-presentation-events.js';
import {applyUnitState,UNIT_STATE_SCHEMA} from './entity-unit-state.js';
import {applyWorldPresentation} from './entity-world-state.js';
import {decodeMountedControl} from './network-mounted-control.js';
import {EntityRemotePresentation} from './network-entity-remotes.js';
import { ItemPresentation } from '../presentation/items.js';
import { CombatPresentation } from './network-entity-combat.js';
import { createCombatRegistry } from './network-combat-registry.js';
import { presentFeedback } from './network-combat-feedback.js';
import { emitSound } from '../audio/events.js';
import { SnapshotClient } from './snapshot-client.js';
import {SnapshotPresentation} from './snapshot-timeline.js';
import { applyOwnPlayer, decodeOwnPlayer } from './network-entity-player.js';
import { ENTITY_POSTURES, ENTITY_SOLDIER_STATES, ENTITY_VEHICLE_STATES } from './network-entity-capture.js';
import { assignUnitId } from './entity-identity.js';
import { applyWorldSummary, decodeWorldSummary } from './network-entity-summary.js';
import { unseatBody } from '../seats.js';
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

// The soldier states that leave him in the air as they are named.
const AIRBORNE = new Set(['jump', 'fall', 'thrown', 'jet']);
// Mount records kept a hull, enough to span the playout delay.
// The shortest way round from one angle (or cycle phase) to another, on a
// circle `length` round.

// The shared gameplay adapter for local and remote servers. Content builds
// models; independently versioned records establish all live world state.
//
// Mounts: a hull is the shared Vehicle the simulation drives; a seat lease is
// the shared SeatLease the SeatReader installs; a turret is the shared Turret
// building. Nothing here duplicates boarding, exit, destruction or aim rules;
// it installs what the server decided and lets the existing presentation
// (the native hull, prop and body phases) draw it.
export class EntityGameState {
  constructor(game, registry, { beforeApply = () => {}, onCorrection = () => {}, onApplied = () => {} } = {}) {
    Object.assign(this, { game, registry, beforeApply, onCorrection, onApplied });
    this.receiver=new SnapshotClient({onRecordsFull:()=>this.releaseRecordPressure()});
    this.by=Array.from({length:NK.SEAT},()=>new Array(NL.RECORDS).fill(null));this.byCounts=new Uint32Array(NK.SEAT);
    this.retrySeatRows=new Array(NL.LEASES).fill(null);this.retrySeatCount=0;this.collectSeat=record=>{if(this.retrySeatCount<this.retrySeatRows.length)this.retrySeatRows[this.retrySeatCount++]=record;};
    this.packetLease=new PacketLease(this.receiver.views);this.correction={record:null,packet:this.packetLease};this.eligibilityPending={record:null,summary:null};
    this.combatContext={epoch:NaN,tick:NaN};this.pendingTick=NaN;
    this.applyStateRecord=(record,id)=>{const applied=record.schema===UNIT_STATE_SCHEMA?applyUnitState(this.game,record,this.registry):applyWorldPresentation(this.game,record);if(applied||this.pendingTick-record.tick>120)this.pendingState.delete(id);};
    this.timeline=this.receiver.network;this.presentation=new SnapshotPresentation(this.timeline,{display:!!game.presentationLoaded});
    const combatRegistry=registry.combat ?? createCombatRegistry(game.data);
    this.combat=new CombatPresentation({ordnance:game.ordnance,definition:id=>combatRegistry.definitions.get(id),predictedShots:game.predictedShots,onFeedback:event=>presentFeedback(game,event),
      muzzle:(event,def)=>{const unit=game.units.byId(event.ownerId);if(!unit||unit.generation!==event.ownerGeneration||unit.removed||unit.seat||!unit.combat?.arsenal)return null;const weapon=unit.combat.arsenal.weapons.find(weapon=>weapon.def?.ordnance===def);if(!weapon)return null;const held=unit.combat.firePoint(weapon)?.position?.clone()??null;if(held)return held;const out=new THREE.Vector3();return unit.soldier?.nextVisualGun?.(out)?out:null;},
      onPresentation:event=>presentEntityEvent(game,event,registry),onExplosion:(def,position)=>game.shakeExplosion?.(def,position)});
    this.items = game.items ? new ItemPresentation(game.items,combatRegistry,game.presentationLoaded) : null;
    this.remotes = game.remotes ? new EntityRemotePresentation(game.remotes,combatRegistry) : null;
    this.ready = false; this.previous = null; this.pendingCorrection = null; this.pendingEligibility = null;
    // Slots named by mount records, so a hull the client has never built can be
    // made from its vehicle record; seats whose mount has not arrived yet.
    this.slots = new RecordMap(); this.pendingSeats = new RecordMap(); this.pendingState = new RecordMap(); this.deferred = 0; this.presentedTick = null;
    // Each hull's recent mount records (lean, walk cycle, seat aims), newest
    // last: drawn at the playout clock with its position (presentMounts).
    this.mounts = new NetworkMounts(this.receiver.network);this.preview=new NetworkPreview(this.receiver.network);
  }
  dispose() {this.dropCorrection();this.dropEligibility();this.slots.clear();this.pendingSeats.clear();this.pendingState.clear();this.presentation.reset();this.timeline.dispose();}
  initialize() {
    const g=this.game;
    // Content constructs the scene; state is supplied solely by the host.
    for(const unit of g.units.all())if(unit.soldier){unit.alive=false;unit.removed=true;unit.actor.root.visible=false;}
    for(const vehicle of g.vehicles?.vehicles || [])this.destroyHull(vehicle);
    this.baseline();
  }
  baseline() { this.slots.clear(); this.pendingSeats.clear(); this.dropCorrection();this.dropEligibility();this.pendingState.clear();this.resetOwnershipProxy(); this.game.ordnance.clearProjectilePresentation(); this.game.items?.clearPresentation(); this.remotes?.reset(null); this.ready = true; }
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
    if (packet.epochChanged) { this.presentation.reset(); this.items?.reset(packet.epoch); this.remotes?.reset(packet.epoch); this.previous = null; this.dropCorrection();this.dropEligibility();this.pendingState.clear();this.slots.clear(); this.pendingSeats.clear(); this.mounts.clear(); }
    // Native publication pairs mount facts directly with their hull. These
    // bounded routing banks retain their backing arrays across every packet.
    const by=this.by,n=this.byCounts;n.fill(0);
    for(let i=0;i<packet.records.length;i++){const row=packet.records.get(i),kind=row.read(NR.KIND);if(kind<NK.SEAT){by[kind][n[kind]++]=row;}}
    const corrected=n[NK.CORRECTION]>0;
    for(let i=0;i<n[NK.CORRECTION];i++)this.prepareOwn(by[NK.CORRECTION][i]);
    for(let i=0;i<n[NK.MOUNT];i++){const row=by[NK.MOUNT][i];if(row.slot!==null)this.slots.set(row.id,row);}
    for(let i=0;i<n[NK.VEHICLE];i++){const row=by[NK.VEHICLE][i];this.vehicle(row,row.paired);}
    for(let i=0;i<n[NK.TURRET];i++)this.turret(by[NK.TURRET][i]);
    for(let i=0;i<n[NK.DROIDEKA];i++)this.droideka(by[NK.DROIDEKA][i]);
    for(let i=0;i<n[NK.SOLDIER];i++)this.soldier(by[NK.SOLDIER][i],corrected);
    for(let i=0;i<n[NK.TOMBSTONE];i++)this.remove(by[NK.TOMBSTONE][i]);
    for(let i=0;i<packet.seatChanges.length;i++)this.seat(packet.seatChanges.get(i));
    this.retrySeatCount=0;this.pendingSeats.forEach(this.collectSeat);
    for(let i=0;i<this.retrySeatCount;i++){const row=this.retrySeatRows[i];this.pendingSeats.delete(row.id);this.seat(row);}
    for(let i=0;i<n[NK.MOUNT];i++)this.mount(by[NK.MOUNT][i]);
    for(let i=0;i<n[NK.SUMMARY];i++){
      const row=by[NK.SUMMARY][i];
      if(row.schema>=6){this.pendingState.set(row.id,row);continue;}
      if(row.schema===3){const summary=decodeWorldSummary(row,this.registry);this.dropEligibility();retainRecord(row);this.eligibilityPending.record=row;this.eligibilityPending.summary=summary;this.pendingEligibility=this.eligibilityPending;}
      else applyWorldSummary(this.game,row,{registry:this.registry});
    }
    for(let i=0;i<n[NK.CORRECTION];i++){
      const row=by[NK.CORRECTION][i];if(!this.pendingCorrection||row.tick>=this.pendingCorrection.record.tick){this.packetLease.capture(packet);this.correction.record=row;this.pendingCorrection=this.correction;}
    }
    this.retryCorrection(packet.tick);this.applyEligibility();this.pendingTick=packet.tick;this.pendingState.forEach(this.applyStateRecord);
    for(let i=0;i<n[NK.REMOTE];i++){if(!this.remotes)throw Error('missing remote presentation');this.remotes.receive(by[NK.REMOTE][i],packet.ack.epoch);}
    for(let i=0;i<n[NK.ITEM];i++){if(!this.items)throw Error('missing item presentation');this.items.receive(by[NK.ITEM][i],packet.ack.epoch);}
    this.combatContext.epoch=packet.ack.epoch;this.combatContext.tick=packet.tick;
    for(let i=0;i<n[NK.COMBAT];i++)this.combat.receive(by[NK.COMBAT][i],this.combatContext);
    this.onApplied(packet);
  }
  dropCorrection(){if(this.pendingCorrection){this.packetLease.release();this.pendingCorrection=null;}this.correction.record=null;}
  dropEligibility(){if(this.pendingEligibility){releaseRecord(this.pendingEligibility.record);this.pendingEligibility=null;}this.eligibilityPending.record=null;this.eligibilityPending.summary=null;}
  // Release expendable deferred history under native pool pressure. Active
  // hull/item facts and resources keep their qualified latest identities.
  releaseRecordPressure(){this.dropCorrection();this.dropEligibility();this.pendingState.clear();this.remotes?.trimHistory();}
  applyEligibility() {
    const pending = this.pendingEligibility, own = this.game.playerUnit;
    if (!pending || pending.summary.playerId !== own.id || pending.summary.generation !== own.generation) return;
    const summary = applyWorldSummary(this.game, pending.record, { registry: this.registry });
    this.eligibility = summary;
    this.dropEligibility();
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
    s.air = state === 'thrownDead' ? 'thrown' : AIRBORNE.has(state) ? state : null;
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
    // A seated body rides its mount (the native body phase);
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
    const slot = metadata?.generation === record.generation ? system.slots?.[metadata.slot] : null;
    if (!className || !slot) { this.deferred++; return null; }
    vehicle = system.variant(slot, className);
    assignUnitId(g.units, vehicle.aiTarget, record.id);
    return vehicle;
  }
  vehicle(record,currentMount) {
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
    if (newLife || !vehicle.tickFrom) vehicle.resetPresentationHistory();
    vehicle.power = record.power; vehicle.root.visible = vehicle.alive;
    vehicle.syncBody(); this.presentation.bindVehicle(record.id, record.generation, vehicle);
    // A mount can be acknowledged before its hull exists. Reuse its retained
    // exact-life pose when the hull arrives; unchanged heartbeats may be deduped.
    const mount = currentMount || this.receiver.get('mount', record.id);
    if (mount?.generation === record.generation) this.mount(mount);
  }
  // A packet only admits an endpoint and seeds a new resource. Numeric
  // history, cyclic angles and own-seat preservation run in the native frame.
  mount(record) {
    const vehicle=this.game.vehicles?.vehicles.find(v=>v.aiTarget.id===record.id);
    if(!vehicle||!vehicle.alive||vehicle.aiTarget.generation!==record.generation)return;
    if(this.ownsMountAim(vehicle,0)){vehicle.walkPhase=record.walkPhase;vehicle.walkBlend=record.walkBlend;}
    if(vehicle.mountGeneration===record.generation&&vehicle.mountEpoch===this.receiver.epoch)return;
    vehicle.mountGeneration=record.generation;vehicle.mountEpoch=this.receiver.epoch;
    // The first endpoint is already a complete pose. Copy it unchanged until
    // the first displayed frame samples the native history.
    if(!this.ownsMountAim(vehicle,0)){
      vehicle.tilt=vehicle.previousTilt=record.tilt;vehicle.roll=vehicle.previousRoll=record.roll;
      vehicle.walkPhase=record.walkPhase;vehicle.walkBlend=record.walkBlend;
    }
    for(let i=0;i<vehicle.seats.length&&i<record.seats.length;i++){
      if(this.ownsMountAim(vehicle,i))continue;
      const seat=vehicle.seats[i];seat.aimYaw=record.seats[i].yaw;seat.pitch=record.seats[i].pitch;
      if(seat.lastYaw===null)seat.lastYaw=seat.aimYaw;
    }
  }
  // Tests and non-drawing readers use the same complete frame operation.
  presentMounts(tick){this.mounts.prepare(this.game,this.previous);this.receiver.network.present(tick);this.mounts.publish();}
  // A stationary turret: its registry identity, build state and the angles the
  // tick left. The drum and gun are posed by the shared Turret.present and
  // the native prop phase; its gunner follows in the body phase.
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
  }
  ownsMountAim(mount, index) {
    const own = this.game.playerUnit, lease = own.seat, previous = this.previous;
    if (!previous || previous.id !== own.id || previous.generation !== own.generation ||
        !lease || lease.released || (lease.vehicle || lease.turret) !== mount || (lease.index || 0) !== index) return false;
    const id = mount.aiTarget?.id, generation = mount.aiTarget?.generation;
    return lease.vehicle && index === 0 && previous.vehicleId === id && previous.vehicleGeneration === generation ||
      previous.mountId === id && previous.mountGeneration === generation && previous.mountSeat === index;
  }
  releasePrerequisite(map,record){const old=map.get(record.id);if(old&&old.generation<=record.generation)map.delete(record.id);}
  remove(record) {
    this.releasePrerequisite(this.slots,record);this.releasePrerequisite(this.pendingSeats,record);this.releasePrerequisite(this.pendingState,record);

    const g = this.game, unit = g.units.byId(record.id); this.presentation.unbind(record.id);
    if (!unit || unit.generation !== record.generation) return;
    this.timeline.remove(record.id);
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
      if (own) { vehicle.seatIndex = seat.seatIndex; reader.setDriver(g.vehicles.vehicles.indexOf(vehicle)); }
      unit.presentation?.reset();
      unit.tickCount += 1;
      return;
    }
    const turret = (g.turretSeats?.turrets || []).find(t => t.aiTarget?.id === seat.vehicleId && t.aiTarget.generation === seat.vehicleGeneration);
    if (!turret) { this.pendingSeats.set(id, { id, generation, seat }); return; }
    if (!turret.alive) return;
    if (turret.lease && turret.lease.unit !== unit) this.clearTurretLease(turret);
    if (own && g.gunner) {
      // A page installs a lease; authority already seated the body. Native
      // presentation consumes it with the gunner's scalar preview angles.
      reader.setLease(turret, null, [id, 1, 'gunner', generation], value => { turret.lease = value; });
      if (g.combat) g.combat.mounted = true;
      g.client.seats.setGunner?.(g.live.buildings.indexOf(turret));
    } else {
      reader.setLease(turret, null, [id, 0, 'gunner', generation], value => { turret.lease = value; });
      if (turret.lease) { turret.lease.generation = generation; }
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
      this.dropCorrection(); return;
    }
    if (this.correct(record, packet)) this.dropCorrection();
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
  // No authority interval advanced: retain the sampled hull/body ring and
  // refresh only own preview and correction queues in one native call.
  presentView(alpha,lockstep,frozen){
    this.preview.prepare(this.game,true,alpha,lockstep,frozen);
    this.receiver.network.presentView();this.preview.publish(this.game);
  }
  present(tick,preview=true,alpha=1,lockstep=false,frozen=false) {
    this.items?.present(tick); this.remotes?.present(tick);
    const g = this.game;
    this.preview.prepare(g,preview,alpha,lockstep,frozen);
    this.mounts.prepare(g,this.previous);
    this.presentation.update(tick, g.playerUnit.id, g.driver?.active ? g.driver.vehicle.aiTarget.id : null);
    this.mounts.publish();
    this.preview.publish(g);
    // Turn-speed facts settle before local preview; native prop and body
    // phases consume the replicated angles and the resulting rider rates.
    const turrets=g.turretSeats?.turrets;
    if(turrets)for (const turret of turrets) {
      const lease = turret.lease; if (!lease || lease.human || !turret.alive) continue;
      turret.settleTurn?.(tick);
    }
  }
}
