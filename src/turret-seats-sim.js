// Turret crews on the simulation module. Bodies and leases keep their
// existing objects; only the riders in this command cross the store.
// Native state is published before callbacks and their effects return
// through a fresh snapshot, including a synchronous death or boarding.
import * as THREE from "three";
import { bridgeFor } from "./sim/runtime.js";
import { makeHit } from "./collision.js";
import { turretState, syncTurretState } from "./turrets-sim.js";
import { putPcInput, PcResultMirror, syncPcResult } from "./sim/gen/props.js";
import { KIND_UNIT, KIND_SEAT_LEASE, KIND_TABLE } from "./sim/gen/index.js";

const EDGES = ["reload", "use", "exit", "jump", "crouch", "prone", "nextPrimary", "previousPrimary", "nextSecondary", "zoom"];
const EMPTY_BUTTONS = [0, 0];
export async function turretSeatClasses() {
  // Load these only when a battle has its classes. Capture installs its
  // seeded weapon hooks before the game loads, including for this path.
  const [{ Unit }, { Soldier }, { Droideka }, { Vehicle }, { Turret }, seats, { hasMissileLock }] = await Promise.all([
    import("./units.js"), import("./soldier.js"), import("./droideka.js"), import("./vehicles.js"), import("./props.js"), import("./seats.js"), import("./weapon-targeting.js"),
  ]);
  return { Unit, Soldier, Droideka, Vehicle, Turret, ...seats, hasMissileLock };
}
export async function attachTurretSeatsSim(instance, crews) {
  return attachTurretSeatsWithClasses(instance, crews, await turretSeatClasses());
}
export function attachTurretSeatsWithClasses(instance, crews, classes) {
  const { Unit, Soldier, Droideka, Vehicle, Turret, SeatLease, SeatUnit } = classes;
  const identity = { classes: { Unit, Soldier, Droideka, Vehicle, Turret, SeatLease, SeatUnit },
    tests: { ownerSnapshot: o => Object.isFrozen(o) && Number.isSafeInteger(o.id) && "className" in o } };
  const bridge = instance.x ? instance.identify(identity) : bridgeFor(instance, identity);
  return new TurretSeatsSim(bridge, crews, classes);
}

export class TurretSeatsSim {
  constructor(bridge, crews, classes) {
    this.bridge = bridge; this.x = bridge.x; this.crews = crews; this.classes = classes; this.contexts = [];
    this.input = { words: this.x.crew_words(0), put: putPcInput };
    this.output = { words: this.x.crew_words(1), Mirror: PcResultMirror, sync: syncPcResult };
    this.hit = makeHit(); this.drop = new THREE.Vector3(); this.turn = new THREE.Quaternion();
    this.pose = { clip: null, pitch: 0, turn: 0, time: null };
    bridge.register({ pcAction: a => this.action(a) });
    crews.seatOf = unit => this.run("seatOf", { unit }).lease;
    crews.canMan = (unit, turret) => this.run("canMan", { unit, turret }).flag;
    crews.man = (unit, turret) => this.run("man", { unit, turret }).flag;
    crews.requestExit = (unit, { post = null, placement = true } = {}) => this.run("requestExit", { unit, post, placement }).flag;
    crews.leave = (unit, { placement = true, post = null } = {}) => this.run("leave", { unit, placement, post }).flag;
    crews.vacate = (lease, { placement = true, post = null, kill = false, attacker = null, reason = "exit" } = {}) => { this.run("vacate", { lease, placement, post, kill, attacker, reason }); };
    crews.destroyed = turret => { this.run("destroyed", { turret }); };
    crews.update = dt => { this.run("update", { dt }); };
    crews.drive = (lease, dt) => { this.run("drive", { lease, dt }); };
    crews.pose = (lease, dt) => { this.run("pose", { lease, dt }); return dt; };
  }
  context() { return this.contexts.at(-1); }
  snapshot() {
    const s = this.crews, ctx = this.context();
    const rows = new Set([...s.leases, ...ctx.leases]);
    const units = new Set(ctx.units);
    for (const t of s.turrets) if (t.lease) rows.add(t.lease);
    for (const t of s.turrets) if (t.gunner) {
      const unit = t.lease?.unit ?? s.units?.all?.().find(u => u.soldier === t.gunner);
      if (unit) units.add(unit);
      else throw Error("a turret gunner must belong to the battle's unit registry");
    }
    for (const lease of rows) if (lease.unit) units.add(lease.unit);
    for (const unit of units) if (unit.seat) rows.add(unit.seat);
    for (const lease of rows) if (lease.unit) units.add(lease.unit);
    for (const unit of units) if (!ctx.mounts.has(unit) && unit.seat?.turret) ctx.mounts.set(unit, unit.seat.turret);
    const controls = [];
    for (const lease of rows) if (lease.controllable && !controls.some(c => c.seatUnit === lease.controllable)) {
      const c = lease.controllable;
      controls.push({ seatUnit: c, driven: Boolean(c.controller), intent: { ...c.intent, buttons: c.intent.buttons || EMPTY_BUTTONS } });
    }
    const state = { time: s.time, ticks: s.ticks, leases: [...s.leases], rows: [...rows], units: [...units],
      turrets: s.turrets.map((t, index) => {
        const center = new THREE.Vector3(), radius = t.sphere(center);
        const gunner = !t.gunner ? null : t.lease?.unit?.soldier === t.gunner ? t.lease.unit : [...units].find(u => u.soldier === t.gunner) ?? null;
        return { state: turretState(t, index), turret: t, alive: t.alive, lease: t.lease, gunner, attacker: t.lastAttacker ?? null, center, radius,
          weapon: Boolean(t.weapon), missileLock: Boolean(t.weapon && this.classes.hasMissileLock(t.weapon.def)), body: t.bodies?.built || null,
          rider: Boolean(t.rider), shootable: Boolean(t.rider?.shootable) };
      }),
      riders: [...units].map(unit => ({ unit, noEnter: Number.parseFloat(unit.classDef?.NoEnterVehicles ?? 0), zoomStep: unit.combat?.zoomStep ?? 0,
        zoom: unit.combat?.zoom ?? 1, firstPerson: Boolean(unit.combat?.firstPerson), blowApart: Boolean(unit.combat?.blowApart),
        dead: Boolean(unit.combat?.dead), mounted: unit.combat?.mounted, gunUnderwater: Boolean(unit.combat?.gunUnderwater), aimOverride: unit.combat?.aimOverride })),
      controls, collide: Boolean(s.collision), latched: [] };
    this.bridge.writeSome(KIND_UNIT, state.units);
    this.bridge.writeSome(KIND_SEAT_LEASE, state.rows);
    return state;
  }
  publish(state) {
    const b = this.bridge, s = this.crews, ctx = this.context(), rows = new Set(state.rows), units = new Set(state.units);
    // Keep both the current command's former riders and its newly made
    // leases available through a nested callback after they leave.
    for (const u of state.units) ctx.units.add(u);
    for (const lease of state.rows) ctx.leases.add(lease);
    b.exported(KIND_SEAT_LEASE, 2, (f, at, h, slot, gen, retired) => {
      const lease = b.refs[KIND_SEAT_LEASE].objects[slot];
      if (!retired && rows.has(lease)) KIND_TABLE[KIND_SEAT_LEASE].sync(b, f, at, h, lease);
    });
    b.exported(KIND_UNIT, 2, (f, at, h, slot, gen, retired) => {
      const unit = b.refs[KIND_UNIT].objects[slot];
      if (!retired && units.has(unit)) KIND_TABLE[KIND_UNIT].sync(b, f, at, h, unit);
    });
    // Combat is deliberately outside Unit's import/mirror rows. The
    // command returns just the flags the seat owns, leaving its facade
    // and the rest of its state with the combat phase.
    for (const fact of state.riders) {
      const combat = fact.unit.combat;
      if (combat) {
        combat.mounted = fact.mounted;
        combat.gunUnderwater = fact.gunUnderwater; combat.aimOverride = fact.aimOverride;
        if ("zoomStep" in combat) combat.zoomStep = fact.zoomStep;
      }
    }
    s.time = state.time; s.ticks = state.ticks;
    s.leases.splice(0, s.leases.length, ...state.leases);
    for (const v of state.turrets) {
      const t = v.turret; syncTurretState(t, v.state); t.lease = v.lease; t.gunner = v.gunner?.soldier ?? null; t.crew = s;
    }
    for (const seat of state.latched) for (const key of EDGES) seat.intent[key] = false;
  }
  run(op, { unit = null, turret = null, lease = null, dt = 0, placement = true, post = null, kill = false, attacker = null, reason = "exit" }) {
    const ctx = { units: new Set(unit ? [unit] : []), leases: new Set(lease ? [lease] : []), mounts: new Map(), shots: [], rounds: [] };
    this.contexts.push(ctx);
    try {
      const input = { op, state: this.snapshot(), unit, turret: this.crews.turrets.indexOf(turret), lease, dt, placement: Boolean(placement), post,
        kill: Boolean(kill), attacker, reason };
      this.bridge.callArgs(this.input, input);
      const result = this.bridge.callResult(this.output, this.x.crew_run()); this.publish(result.state); return result;
    } finally { this.contexts.pop(); }
  }
  posed(lease) {
    const unit = lease.unit, turret = lease.turret, point = unit.soldier.position;
    unit.presentation?.reset?.();
    if (unit.actor?.root) { unit.actor.root.position.copy(point); unit.actor.root.visible = !turret.hidesGunner; }
    const pose = turret.riderSample?.(unit.animator?.clips, this.pose);
    unit.animator?.gunner?.(pose?.clip ?? null, pose?.pitch, pose?.turn, pose?.time, turret.seatTurn?.(this.turn) ?? null);
  }
  action(a) {
    const ctx = this.context(), s = this.crews, w = s.world;
    // The import mirror can be reused by a reentrant command. Copy every
    // primitive and vector before a callback, and keep its object refs.
    const { kind, unit, lease, value, yaw, flag, second, third, body, attacker, post, reason, index, shot } = a;
    const t = s.turrets[a.turret], point = a.point.clone(), direction = a.direction.clone();
    const result = { state: null, lease: null, control: null, hit: null, flag: false, value: undefined, point: new THREE.Vector3(), shots: [] };
    switch (kind) {
      case "publish": this.publish(a.state); break;
      case "resume": result.state = this.snapshot(); break;
      case "makeLease": {
        const m = a.made;
        const lease = new this.classes.SeatLease({ unit: m.unit, human: m.human, team: m.team, vehicle: m.vehicle, turret: m.turret,
          index: m.index, role: m.role, time: m.since, tick: m.tick });
        lease.generation = m.generation; ctx.leases.add(lease);
        this.bridge.writeSome(KIND_SEAT_LEASE, [lease]); result.lease = lease; break;
      }
      case "cast": {
        const h = flag ? s.collision.cast(point, direction, value, this.hit) : w.cast(point, direction, value, s.hit, body);
        result.hit = { found: Boolean(h), distance: h?.distance ?? 0, point: h?.point ?? point, normal: h?.normal ?? direction, terrain: Boolean(h?.terrain), body: h?.body ?? null }; break;
      }
      case "overlaps": result.flag = w.overlapsSphere(point, value, body); break;
      case "raycast": result.value = w.raycast(point, direction, value, unit, s.hit)?.distance; break;
      case "kill": w.kill(unit, attacker); break;
      case "riderDrop": { const p = w.riderDrop(t, index, unit, this.drop); result.flag = Boolean(p); if (p) result.point.copy(p); break; }
      case "controllerState": result.value = w.controllerState(unit); break;
      case "controllable": result.control = w.controllable(lease); break;
      case "boarded": w.boarded(unit, t, index); break;
      case "unboarded": w.unboarded(unit, t, index, post, reason); break;
      case "seated": if (unit.combat) unit.combat.zoomed = false; break;
      case "unseated":
        if (unit.combat) unit.combat.zoomed = false;
        unit.animator?.gunner?.(null); unit.actor?.root?.rotation?.set?.(0, yaw, 0);
        if (ctx.mounts.get(unit)?.hidesGunner && unit.actor?.root && !unit.removed && !unit.combat?.blowApart) unit.actor.root.visible = true;
        break;
      case "placed":
        unit.presentation?.reset?.(); unit.animator?.gunner?.(null); unit.animator?.reset?.(yaw);
        if (unit.actor?.root) { unit.actor.root.visible = true; unit.actor.root.rotation?.set?.(0, yaw, 0); } break;
      case "dropped": unit.presentation?.reset?.(); unit.actor?.root?.position?.copy?.(point); break;
      case "seatPolicy": this.classes.seatWeaponPolicy(t.weapon, flag, second ? s.seatRandom : null); break;
      case "pose": this.posed(lease); break;
      case "lock": w.updateLock(t.weapon, value, point, direction, t.aiTarget, t.bodies?.built, lease); break;
      case "weapon":
        ctx.shots = w.weaponUpdate(t.weapon, value, true, { held: flag, pressed: second }, third, { stick: 0, posture: "stand", zoomed: false });
        result.shots = ctx.shots.map(s => ({ yaw: s.yaw ?? 0, pitch: s.pitch ?? 0 })); break;
      case "fire": {
        const round = w.fire(t.weapon.def.ordnance, point, direction, unit, t.bodies?.built || null, lease.emitter, ctx.shots[shot]);
        result.value = ctx.rounds.push(round) - 1; break;
      }
      case "bind": w.bindRound(ctx.rounds[index], t, -1, -1, value); break;
      case "sound": w.sound("turretFire", t, ctx.shots[shot]); break;
      case "fired": w.fired(t, 0, ctx.shots[shot], unit, point, direction); break;
      default: throw Error(`unknown turret crew action ${kind}`);
    }
    return result;
  }
}
