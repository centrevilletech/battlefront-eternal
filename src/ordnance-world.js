import { resolveOrdnanceOwner } from "./ordnance-owner.js";
import { legacyRandom } from "./ai/random.js";
import { strikeable } from "./registry.js";
import { networkShotFor } from "./runtime/network-shot-aim.js";
import { navigateMissile } from "./flight/targeting.js";
import { emitSound } from "./audio/events.js";
import { blastBlocked } from "./ordnance.js";

// What the rounds in flight (src/ordnance.js) and the things lying about
// (src/items.js) ask of the rest of the battle: every read of state they do
// not own (units, hulls, the collision world, the water, the hit history)
// and every call into code that is not theirs (damage, explosions, the
// capture's observers, the random streams). Their own logic reaches the
// battle through these calls and nothing else, in the order it makes them,
// so this is the boundary the simulation's port to Rust moves them across
// (docs/sim-store.md): the same calls are the trait the Rust ordnance
// reaches the battle through, what the replay harness records of each frame
// of theirs (tools/sim-capture/specs.mjs), and, once Rust owns the rounds,
// what its module calls back into JavaScript.
//
// What only draws (effects, lights, a model's pose, sounds no capture
// hears) is not here: a world draws nothing, and the page draws the rounds
// from their records.

// Draws with no generator given (spinRandom, random null): Math.random.
const legacySpin = legacyRandom("grenade-spin");
const legacyWaver = legacyRandom("missile-waver");
const legacyDrop = legacyRandom("item-drop");

export class OrdnanceWorld {
  constructor(ordnance) {
    this.ordnance = ordnance;
    // The round a unit raycast is for, for its filter (mayStrike).
    this.striking = { owner: null, body: null, mount: null, filter: (unit) => this.mayStrike(this.striking.owner, unit, this.striking.body) };
    // A hull a round may strike a rider through (riderMount).
    this.mounted = { vehicle: null, boundRadius: 0, body: null };
    // Whom a shot is credited to (shooter).
    this.shot = { fireId: 0, unit: 0, generation: 0 };
    this.near = [];
  }

  // The collision world's casts (src/collision.js), into `out`.
  cast(origin, direction, far, out, ignoreBody, staticOnly) {
    return this.ordnance.collision.cast(origin, direction, far, out, ignoreBody, staticOnly);
  }

  castMoving(origin, direction, far, out, ignoreBody) {
    return this.ordnance.collision.castMoving?.(origin, direction, far, out, ignoreBody);
  }

  // The first unit a segment meets (Units.raycast, or the hit history's
  // past one for a shot judged at `historyTick`), other than the round's
  // owner's life and those mayStrike rules out; null if none, or if the
  // battle has no units. `mount`: the hull a round has gone through to its
  // riders (riderMount), whose riders alone it may strike.
  raycastUnits(origin, direction, far, owner, historyTick, ignoreBody, mount) {
    const o = this.ordnance;
    if (!o.units) return null;
    const shooter = owner && typeof owner === "object" ? resolveOrdnanceOwner(owner) : null;
    const striking = this.striking;
    striking.mount = mount;
    let filter = null;
    if (o.canHit || ignoreBody || mount) {
      striking.owner = owner;
      striking.body = ignoreBody;
      filter = striking.filter;
    }
    try {
      if (historyTick != null && o.hitHistory) return o.hitHistory.raycast(historyTick, origin, direction, far, shooter, o.unitHit, filter, true);
      return o.units.raycast(origin, direction, far, shooter, o.unitHit, filter, true);
    } finally {
      striking.mount = null;
    }
  }

  // Whether a round may strike this unit: not a rider on the hull that fired
  // it, only a rider of the mount it is passing through (riderMount), and
  // whatever the battle's rules (canHit) allow.
  mayStrike(owner, unit, ignoreBody = null) {
    if (ignoreBody && unit.seat && (unit.seat.vehicle?.body ?? unit.seat.turret?.bodies?.built) === ignoreBody) return false;
    if (this.striking.mount && unit.seat?.vehicle !== this.striking.mount) return false;
    return !this.ordnance.canHit || this.ordnance.canHit(owner, unit);
  }

  // The hull a round that met `body` may strike a rider through: one whose
  // collision doesn't keep rounds off its riders (Vehicle.shieldsRiders, a
  // riding animal's) and that has riders. { vehicle, boundRadius, body }, or
  // null.
  riderMount(body) {
    const mount = body?.owner;
    if (!mount || mount.shieldsRiders?.() !== false || !mount.occupied) return null;
    const m = this.mounted;
    m.vehicle = mount;
    m.boundRadius = mount.boundRadius;
    m.body = mount.body;
    return m;
  }

  // The units a fat ray's sweep may strike near `center` (Units.query):
  // soldiers on foot and riders in the open, not its owner, none it has
  // struck, none the rules spare. The list is this object's, reused.
  fatQuery(center, radius, owner, struck, ignoreBody) {
    const units = this.ordnance.units;
    if (!units?.query) return null;
    return units.query(center, radius, this.near,
      (unit) => unit !== owner && !struck.has(unit) && strikeable(unit) && this.mayStrike(owner, unit, ignoreBody));
  }

  // A unit's bounding sphere ({ center, radius }), made on first asking in
  // a tick (src/units.js).
  unitSphere(unit) {
    return unit.sphere;
  }

  // Whether a unit's class takes hits as a droid (its HealthType).
  isDroid(unit) {
    return (unit.classDef?.HealthType || "").toLowerCase() === "droid";
  }

  // Whether an owner or a beam's source is gone (alive is false: a unit
  // dead or a snapshot of a life that ended, a hull destroyed).
  gone(x) {
    return x?.alive === false;
  }

  // A beam's start in its source's frame, for the beam to follow it, or null
  // for a source that does not move.
  beamLocalOrigin(source, origin) {
    return source?.matrix ? origin.clone().applyMatrix4(source.matrix.clone().invert()) : null;
  }

  // A following beam's start and direction now: its local start through the
  // source's frame, and the source's view direction where it has one.
  beamPose(source, localOrigin, position, direction) {
    position.copy(localOrigin).applyMatrix4(source.matrix);
    source.viewDirection?.(direction);
  }

  // The tick a lag-compensated shot meets the others at (HitHistory
  // shotTick): null for one judged now, undefined in a battle without one.
  shotTick(owner, channel) {
    return this.ordnance.hitHistory?.shotTick(owner, channel);
  }

  // The hit history's tick (the world's), up to which a lag-compensated
  // bolt flies at once.
  historyTick() {
    return this.ordnance.hitHistory.tick;
  }

  // Whom a bolt is credited to on the wire: the press it answers (its id),
  // and the owner's unit and life (0 for none).
  shooter(owner, channel) {
    const s = this.shot;
    s.fireId = networkShotFor(owner, channel)?.id ?? 0;
    s.unit = owner?.id ?? 0;
    s.generation = owner?.generation ?? 0;
    return s;
  }

  // Where a guided missile's target is aimed at now (into `out`), if it is
  // still the life it was fired at; false if not.
  missileTarget(targetId, targetGeneration, out) {
    const target = this.ordnance.units?.byId?.(targetId);
    if (!(target?.alive && (target.generation ?? 0) === targetGeneration)) return false;
    if (target.aimPoint) target.aimPoint(out);
    else out.copy(target.position);
    return true;
  }

  // A flyer's missile steered by its seeker (src/flight/targeting.js
  // navigateMissile): turns `velocity` in place; returns its target's id,
  // -1 once the seeker has lost it.
  navigateMissile(position, velocity, flightClass, targetId, targetGeneration, dt) {
    const target = this.ordnance.units?.byId?.(targetId);
    if (!(target?.alive && (target.generation ?? 0) === targetGeneration)) return targetId;
    const missile = { position, velocity, flightClass, targetId };
    navigateMissile(missile, target, dt);
    return missile.targetId;
  }

  // Where a segment goes down into the water (into `out`), or null.
  waterCrossing(from, to, out) {
    const water = this.ordnance.water;
    return water ? water.crossing(from, to, out) : null;
  }

  // Draws: a missile's waver (the battle's gameplay stream) and a grenade's
  // tumble (its grenade-spin stream).
  waver() {
    return (this.ordnance.random || legacyWaver)();
  }

  spin() {
    return (this.ordnance.spinRandom || legacySpin)();
  }

  // An explosion (Ordnance.explosion): its look, then its damage, push and
  // shake to everything near (the battle's onExplode).
  explosion(def, position, owner) {
    this.ordnance.explosion(def, position, owner);
  }

  // What a round did, to whom it was done: damage to a body's owner (a
  // building, a hull) and to a unit.
  hit(body, def, owner, direction) {
    this.ordnance.onHit(body, def, owner, direction);
  }

  hitUnit(unit, def, point, owner, part, direction) {
    this.ordnance.onHitUnit(unit, def, point, owner, part, direction);
  }

  // The capture's observers: a round made, a bolt launched, a round's end.
  projectile(round, type) {
    this.ordnance.onProjectile(round, type);
  }

  launch(bolt, tick) {
    this.ordnance.onLaunch(bolt, { tick });
  }

  impact(round, hit) {
    this.ordnance.onImpact(round, hit);
  }

  // A grenade's first touch of anything (the bots' danger warnings).
  grenadeContact(grenade) {
    this.ordnance.onGrenadeContact(grenade);
  }

  // A grenade's bounce, which the presentation capture hears.
  bounce(def, position, speed) {
    emitSound("bounce", def, position, speed);
  }
}

// The same for the things lying about (src/items.js).
export class ItemsWorld {
  constructor(items) {
    this.items = items;
    // Who may take power-ups or set off mines this update (Items.update's
    // targets).
    this.targets = [];
    this.watched = { relation: 0, center: null, radius: 0, velocity: null };
    this.ownerIds = new WeakMap();
    this.nextOwnerId = 1;
  }

  cast(origin, direction, far, out) {
    return this.items.collision.cast(origin, direction, far, out);
  }

  // Whether terrain or a building stands between a blast and a point.
  blocked(position, target) {
    return blastBlocked(this.items.collision, position, target);
  }

  waterCrossing(from, to, out) {
    return this.items.water?.crossing(from, to, out) ?? null;
  }

  // An item's model, made to show it: its mesh (null without one), its
  // size and middle (the largest of its parts' bounding spheres) for blasts
  // and pickups.
  model(def) {
    return this.items.presentationModel(def);
  }

  // An owner's part of a dispenser's key (the life that threw: the same
  // unit's next life keeps its own items).
  ownerKey(owner) {
    if (!owner || typeof owner !== "object") return String(owner);
    if (!this.ownerIds.has(owner)) this.ownerIds.set(owner, this.nextOwnerId++);
    return `unit:${this.ownerIds.get(owner)}:${owner.generation ?? 0}`;
  }

  // The drop roll at a death.
  dropRoll() {
    return (this.items.random || legacyDrop)();
  }

  explode(def, position, owner) {
    this.items.explode(def, position, owner);
  }

  // A target's side toward a mine's team (below 0 an enemy, above 0 a
  // friend, 0 neither) and, unless neither, where it is, its size and its
  // velocity now.
  watch(index, team) {
    const target = this.targets[index], w = this.watched;
    w.relation = target.relation(team);
    if (w.relation === 0) return w;
    w.center = target.center;
    w.radius = target.radius;
    w.velocity = target.velocity;
    return w;
  }

  // Where a target stands, for a power-up.
  standing(index) {
    return this.targets[index].position;
  }

  // A power-up given: whether the target took anything.
  give(index, def) {
    return this.targets[index].give(def);
  }

  // A power-up taken, which the presentation capture hears.
  pickup(def, position) {
    emitSound("pickup", def, position);
  }
}
