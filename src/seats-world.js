import { updateWeaponLock, seatLockTarget } from "./weapon-targeting.js";
import { bindMountedRound } from "./muzzles.js";
import { emitSound } from "./audio/events.js";
import { SeatUnit } from "./seats.js";

// What the seats (src/seats.js TurretSeats, the hulls' seats in
// src/vehicles.js Vehicles, src/boarding.js) ask of the rest of the battle
// as people board, sit, aim, fire and get out: the collision world's and the
// registry's queries, what only the units' code does to a body (standing it
// up, killing it), a rider's drop point from his pose, the weapons' and the
// rounds' code, the battle's hooks and the AI's controls. Their own logic
// reaches the battle through these calls and nothing else, in the order it
// makes them, so this is the boundary the simulation's port to Rust moves
// them across (docs/sim-store.md): the same calls are the trait the Rust
// seats reach the battle through, what the replay harness records of their
// frames (tools/sim-capture/specs.mjs), and, once Rust owns the seats, what
// its module calls back into JavaScript.
//
// A seat system's own state (its leases, the hulls' seats and the turrets'
// aim) and the fields of a body that only the seats write (its seat, its
// pose at a turret, its place when it gets out) are read and written
// directly; so is what only a page draws (a body's presentation, its
// animator's gunner pose, its model's root).

export class SeatsWorld {
  // system: the TurretSeats or the Vehicles whose seats these are (its
  // collision, registry, rounds and hooks).
  constructor(system) {
    this.system = system;
    // relation() as a function of two teams (src/boarding.js boardingSeat),
    // reaching the method as it stands when called.
    this.relate = (a, b) => this.relation(a, b);
  }

  // The collision world's first hit along a ray (CollisionWorld.cast), into
  // `out`, leaving out ignoreBody; null for none.
  cast(origin, direction, far, out, ignoreBody) {
    return this.system.collision.cast(origin, direction, far, out, ignoreBody);
  }

  // Whether a sphere overlaps anything solid but ignoreBody
  // (CollisionWorld.overlapsSphere).
  overlapsSphere(center, radius, ignoreBody) {
    return this.system.collision.overlapsSphere(center, radius, ignoreBody);
  }

  // The first unit along a ray within `far` (Units.raycast), `exclude` left
  // out, its hit in `out`; null for none or a battle without units.
  raycast(origin, direction, far, exclude, out) {
    return this.system.units?.raycast?.(origin, direction, far, exclude, out);
  }

  // Two teams' relation (1 friend, 0 neutral, -1 enemy): the registry's,
  // with its alliances, where the system has one.
  relation(a, b) {
    return this.system.relation(a, b);
  }

  // A body stood up at a point, facing yaw (Soldier.spawn: on whatever lies
  // below, at rest, out of the air).
  standUp(unit, x, y, z, yaw) {
    unit.soldier.spawn(x, y, z, yaw);
  }

  // A unit killed outright, credited to attacker (its combat's damage).
  kill(unit, attacker) {
    unit.combat?.damage?.(Infinity, Infinity, null, attacker);
  }

  // Where a rider killed on seat `index` of a hull or turret starts to fall
  // from (his pose's drop point, src/rider-pose.js), into out; null where
  // none shows.
  riderDrop(holder, index, unit, out) {
    return holder.riderDrop?.(index, unit, out) ?? null;
  }

  // The AI controller's state of a unit (5: boarding something else, which
  // keeps the seat it leaves from being handed on), or undefined.
  controllerState(unit) {
    return unit?.controller?.state;
  }

  // The seat as the AI drives it (src/seats.js SeatUnit), made for a bot's
  // lease each time it takes a seat.
  controllable(lease) {
    return new SeatUnit(lease);
  }

  // A launcher's lock as a seat's or turret's bot holds it
  // (updateWeaponLock), its target the bot's own (seatLockTarget).
  updateLock(weapon, dt, origin, dir, viewer, ignoreBody, lease) {
    updateWeaponLock(weapon, dt, { units: this.system.ordnance?.units, collision: this.system.collision,
      origin, dir, viewer, ignoreBody, target: seatLockTarget(lease) });
  }

  // A weapon's tick (Weapon.update): the shots it fires.
  weaponUpdate(weapon, dt, selected, trigger, reload, options) {
    return weapon.update(dt, selected, trigger, reload, options);
  }

  // A round of `def` (Ordnance.fire): a rocket's, missile's or shell's
  // record, nothing for the rest.
  fire(def, origin, direction, owner, ignoreBody, emitter, shot) {
    return this.system.ordnance?.fire(def, origin, direction, owner, null, ignoreBody, emitter, shot);
  }

  // Which mount, seat, weapon and barrel a round is drawn leaving
  // (bindMountedRound).
  bindRound(round, mount, seat, weapon, barrel) {
    bindMountedRound(round, mount, seat, weapon, barrel);
  }

  // A sound (the presentation capture hears turretFire).
  sound(type, ...args) {
    emitSound(type, ...args);
  }

  // The battle's hooks: someone took a seat, left one, moved to another, or
  // a seat fired.
  boarded(unit, holder, index) {
    this.system.onBoard?.(unit, holder, index);
  }

  unboarded(unit, holder, index, post, reason) {
    this.system.onUnboard?.(unit, holder, index, post, reason);
  }

  moved(unit, holder, index) {
    this.system.onMove?.(unit, holder, index);
  }

  fired(holder, seat, shot, owner, muzzle, direction) {
    this.system.onFired?.(holder, seat, shot, owner, muzzle, direction);
  }
}
