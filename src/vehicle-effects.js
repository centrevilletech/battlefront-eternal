import * as THREE from "three";
import { Footfalls } from "./vehicle-feet.js";

// What a hull shows beyond its model, at Classic (docs/vehicles.md,
// "Effects and sounds"), from its class's fx (tools/zero_vehicle.py
// vehicle_fx):
//
// - Thrust: each ThrustEffect burns at its attach point and offset from the
//   moment a flyer starts to take off until it has landed. Its particles
//   are drawn at a scale that rests at ThrustEffectMinScale and, once the
//   hull's speed passes the point ThrustEffectScaleStart of the way from
//   MinSpeed to MaxSpeed, grows in step with it, reaching
//   ThrustEffectMaxScale at MaxSpeed. The original game doesn't stop it there,
//   but its flyers never go much faster; the port's boost does, so here it
//   stops at ThrustEffectMaxScale.
// - Damage: each damage effect burns at its attach point while the hull's
//   health is at most DamageStartPercent and above DamageStopPercent of its
//   MaxHealth, at DamageEffectScale, its particles carrying
//   DamageInheritVelocity of the hull's velocity.
// - Water: a hover's WaterEffect, on the water's surface under it while it
//   is within WATER_HEIGHT of it.
// - Stomp: a walker's StompEffect where a foot comes down (vehicle-feet.js),
//   at the foot level with the walker's root, turned as the foot is.
// - Death: the hull's effects stop and its chunks fly off (Debris), with
//   their trails and each one's ChunkSmokeEffect. A walker with a death
//   animation is dying first (Vehicle.dying, the simulation's): every
//   DeathAnimationExplosionTime one of its DeathAnimationExplosion effects
//   (picked at random) goes off at a node of its model picked at random,
//   its DeathDustEffect rises at DeathDustOffset DeathDustDelay after it
//   was destroyed if it is still dying then (the original game runs the
//   dust's clock only while the animation plays), and its chunks fly when
//   it is gone, with its FinalExplosion (the simulation's blast).
//
// Placed turrets and destructible posts (src/props.js) go through the same
// rules (updateBuildings): their damage effects while built, at the built
// model's nodes, a post's death animation as a walker's (with its
// DeathEffects at their hardpoints as it ends), and their chunks when
// destroyed (tools/extract.py write_props
// gives them a hull's fx; the building reader takes those keys as a hull's).
//
// It only draws: it reads what every host gives a hull (alive, dying,
// health, flight state, velocity and the pose drawn this frame) or a
// building (its state and health) and runs on the page, so a battle
// simulated here, in a worker or on a server looks the same. Missing
// attach points fall back to the hull's root, as the original game's do.

const ACTIVE_FLIGHT = new Set(["TakingOff", "Flying", "Landing"]);
const WATER_HEIGHT = 4; // m above the surface a hover still raises spray (the port's)
const CHUNK_OVERKILL = -50; // the health Debris flings pieces at full speed for
const DYING_INTERVAL = 0.4; // s, DeathAnimationExplosionTime unless set
const _matrix = new THREE.Matrix4();
const _offset = new THREE.Matrix4();
const _position = new THREE.Vector3();
const _origin = new THREE.Vector3();
const _quaternion = new THREE.Quaternion();
const _scale = new THREE.Vector3();
const _velocity = new THREE.Vector3();
const _bind = new THREE.Matrix4();
const _local = new THREE.Matrix4();
const UP = new THREE.Vector3(0, 1, 0);
const STILL = Object.freeze(new THREE.Vector3());

// A placed turret as the rules above see a hull: alive while built,
// standing still, its nodes the built model's, a new life each time it is
// built again (its health then fills up from the wreck's).
class BuildingView {
  constructor(building) {
    const rig = building.looks?.built || building.rig || null;
    this.building = building;
    this.def = { fx: building.desc?.fx };
    this.root = rig?.root ?? null;
    this.byName = rig?.byName || {};
    this.bones = rig?.bones || [];
    this.velocity = STILL;
    this.aiTarget = { generation: 0 };
    this.built = false;
  }

  // The collision it has now (a wreck's, once destroyed): its pieces start
  // inside it.
  get body() { return this.building.bodies?.[this.building.state] ?? null; }
  get alive() { return this.building.alive; }
  get health() { return this.building.health; }
  get maxHealth() { return this.building.maxHealth; }
  // A destructible post's death animation left (src/props.js ObjectivePost).
  get dying() { return this.building.dying ?? 0; }

  // Before each frame: a building built since the last is a new life.
  next() {
    const built = this.building.alive;
    if (built && !this.built) this.aiTarget.generation += 1;
    this.built = built;
    return this;
  }
}

// The thrust scale for a speed, from one of the class's ThrustEffect
// entries and its [MinSpeed, MaxSpeed].
export function thrustScale(entry, speed, [min, max]) {
  const top = max || 1;
  const start = (top - min) * entry.scaleStart + min;
  const span = top - start;
  const t = span > 1e-6 ? THREE.MathUtils.clamp((speed - start) / span, 0, 1) : speed > start ? 1 : 0;
  return entry.minScale + (entry.maxScale - entry.minScale) * t;
}

// Whether a damage entry burns at a health (both in hit points).
export function damageBurns(entry, health, maxHealth) {
  return health > entry.stop * maxHealth && health <= entry.start * maxHealth;
}

// Where a dead walker's pieces start. They are modelled in its standing
// pose, in the hull's frame; its death animation has since brought its body
// down (the AT-TE's by 2.5 m, onto its belly). Carried by its body bone
// (bone_root) from where that pose has it to where the animation left it,
// they start where the body lies instead of jumping back up to where it
// stood; a piece that would start below the ground starts on it (Debris,
// floor). Null for a hull without that bone (the root carries its pieces).
const _carrier = new THREE.Object3D();
_carrier.matrixAutoUpdate = false;
export function bodyCarrier(v) {
  const body = v.byName?.bone_root;
  if (!body || !v.bones || !v.bindQuaternions || !v.bindPositions) return null;
  _bind.identity();
  for (let bone = body; bone !== v.root; bone = bone.parent) {
    const i = bone ? v.bones.indexOf(bone) : -1;
    if (i < 0) return null;
    _bind.premultiply(_local.compose(v.bindPositions[i], v.bindQuaternions[i], bone.scale));
  }
  body.updateWorldMatrix(true, false);
  _carrier.matrix.copy(body.matrixWorld).multiply(_local.copy(_bind).invert());
  _carrier.matrixWorld.copy(_carrier.matrix);
  return _carrier;
}

export class VehicleEffects {
  constructor({ effects, water = null, debris = null, random = Math.random }) {
    this.effects = effects;
    this.water = water;
    this.debris = debris;
    this.random = random; // the dying explosions' picks (the page's own)
    this.states = new WeakMap();
    this.views = new WeakMap(); // building -> BuildingView
  }

  update(vehicles, dt) {
    for (const v of vehicles || []) this.updateVehicle(v, dt);
  }

  // The placed turrets (src/props.js LiveObjects.turrets): those whose
  // class has damage effects or chunks.
  updateBuildings(buildings, dt) {
    for (const building of buildings || []) {
      if (!building.desc?.fx) continue;
      let view = this.views.get(building);
      if (!view) this.views.set(building, (view = new BuildingView(building)));
      if (view.root) this.updateVehicle(view.next(), dt);
    }
  }

  state(v) {
    let s = this.states.get(v);
    if (!s) this.states.set(v, (s = { life: -1 }));
    if (s.life !== v.aiTarget?.generation) {
      this.stopAll(s);
      Object.assign(s, { life: v.aiTarget?.generation, alive: v.alive, thrust: [], damage: [], water: null, feet: new Footfalls(), dust: null,
        dying: null });
    }
    return s;
  }

  stopAll(s) {
    for (const h of s.thrust || []) h?.stop();
    for (const h of s.damage || []) h?.stop();
    s.water?.stop();
    if (s.thrust) s.thrust.length = 0;
    if (s.damage) s.damage.length = 0;
    s.water = null;
  }

  updateVehicle(v, dt) {
    const fx = v.def?.fx;
    if (!fx) return;
    const s = this.state(v);
    if (!v.alive) {
      if (s.alive) this.destroyed(v, s, fx);
      s.alive = false;
      if (s.dying) this.updateDying(v, s, fx);
      this.updateDust(v, s, fx, dt);
      return;
    }
    s.alive = true;
    if (fx.thrust) this.updateThrust(v, s, fx);
    if (fx.damage) this.updateDamage(v, s, fx);
    if (fx.water && this.water) this.updateWater(v, s, fx);
    if (fx.stomp?.effect) this.updateStomp(v, s, fx, dt);
  }

  // The frame an entry hangs from: its node (else the hull's root) and
  // offset, in the world as drawn this frame.
  place(v, node, offset) {
    const bone = (node && v.byName?.[node]) || v.root;
    bone.updateWorldMatrix(true, false);
    _matrix.copy(bone.matrixWorld);
    if (offset) _matrix.multiply(_offset.makeTranslation(offset[0], offset[1], offset[2]));
    _matrix.decompose(_position, _quaternion, _scale);
  }

  updateThrust(v, s, fx) {
    const on = v.flying && ACTIVE_FLIGHT.has(v.flightState);
    const speed = v.velocity.length();
    fx.thrust.forEach((entry, i) => {
      let handle = s.thrust[i];
      if (!on) {
        handle?.stop();
        s.thrust[i] = null;
        return;
      }
      this.place(v, entry.node, entry.offset);
      if (!handle || handle.done) handle = s.thrust[i] = this.effects.play(entry.effect, _position, null, v.velocity, _quaternion);
      else handle.move(_position, _quaternion, v.velocity);
      if (handle) handle.scale = thrustScale(entry, speed, fx.speed || [10, 30]);
    });
  }

  updateDamage(v, s, fx) {
    fx.damage.forEach((entry, i) => {
      let handle = s.damage[i];
      if (!damageBurns(entry, v.health, v.maxHealth)) {
        handle?.stop();
        s.damage[i] = null;
        return;
      }
      this.place(v, entry.node, entry.offset);
      _velocity.copy(v.velocity);
      if (!handle || handle.done) {
        handle = s.damage[i] = this.effects.play(entry.effect, _position, null, _velocity, _quaternion);
        if (handle) { handle.scale = entry.scale; handle.inherit = entry.inherit; }
      } else handle.move(_position, _quaternion, _velocity);
    });
  }

  updateWater(v, s, fx) {
    const p = v.root.position;
    const level = this.water.level;
    const over = Number.isFinite(level) && this.water.over?.(p.x, p.z) && p.y - level < WATER_HEIGHT && p.y > level - WATER_HEIGHT;
    if (!over) {
      s.water?.stop();
      s.water = null;
      return;
    }
    _position.set(p.x, level, p.z);
    if (!s.water || s.water.done) s.water = this.effects.play(fx.water, _position, UP, v.velocity);
    else s.water.move(_position, null, v.velocity);
  }

  // Where a foot comes down (src/vehicle-feet.js): its StompEffect at the
  // foot, level with the walker's root, turned as its primitive is (the
  // walker's foot update plays it so).
  updateStomp(v, s, fx, dt) {
    s.feet.update(v, fx.feet || [], dt, (i, point, step, turn) => {
      if (step !== "down") return;
      _position.set(point.x, v.root.matrixWorld.elements[13], point.z);
      this.effects.play(fx.stomp.effect, _position, UP, null, turn);
    });
  }

  // Destroyed: its effects out; a dying walker's pieces wait for its end.
  destroyed(v, s, fx) {
    this.stopAll(s);
    s.dying = v.dying > 0 && fx.dying ? { fired: 0, dust: !fx.deathDust } : null;
    s.dust = fx.deathDust && !s.dying ? { left: fx.deathDust.delay || 0 } : null;
    if (!s.dying) this.blowApart(v, fx);
  }

  // dead: a walker whose death animation has played (updateDying).
  blowApart(v, fx, dead = false) {
    if (!fx.chunks?.length || !this.debris || !v.root) return;
    v.root.updateMatrixWorld(true);
    // A chunk without a node of its own is modelled in the hull's frame, and
    // so is one naming a node the model lacks (the spider walker's name the
    // model itself), as attach points fall back to the root. The pieces fly
    // at their full ChunkSpeed (a droid's pieces slow with how little it was
    // overkilled; a hull's pace isn't traced), each from its own middle
    // (Debris, centre).
    const hull = (dead && bodyCarrier(v)) || v.root;
    const bones = { ...v.byName, "": hull };
    for (const chunk of fx.chunks) {
      const node = String(chunk.ChunkNodeName || "").toLowerCase();
      if (!bones[node]) bones[node] = hull;
    }
    _origin.setFromMatrixPosition(v.root.matrixWorld);
    // None starts below the ground the hull stood on (a leg's piece, carried
    // down with a fallen body).
    this.debris.blowApart(fx.chunks, bones, _origin, v.velocity, CHUNK_OVERKILL, v.body ?? null, { centre: true, floor: _origin.y });
  }

  // A frame of a dying walker (the hull drawn in its death animation,
  // Vehicle.present), timed by the simulation's dying clock, so a page
  // drawing slowly, or a worker's or a server's hull, sees the same: an
  // explosion each time another interval has passed (at most one a frame,
  // as the original game's timer), at a node picked at random; its dust once
  // its delay has passed; its pieces once it is gone.
  updateDying(v, s, fx) {
    if (!(v.dying > 0)) {
      s.dying = null;
      // An animated building's DeathEffects, each at its hardpoint, as its
      // death clip ends.
      for (const entry of fx.dying?.final || []) {
        this.place(v, entry.node, null);
        this.effects.play(entry.effect, _position, null, null, _quaternion);
      }
      this.blowApart(v, fx, true);
      return;
    }
    const dying = fx.dying;
    const elapsed = Math.max((dying.span || 0) - v.dying, 0);
    if (!s.dying.dust && elapsed > (fx.deathDust.delay || 0)) {
      s.dying.dust = true;
      this.place(v, null, fx.deathDust.offset);
      this.effects.play(fx.deathDust.effect, _position, UP);
    }
    const due = Math.floor(elapsed / Math.max(dying.interval ?? DYING_INTERVAL, 1e-3));
    if (!dying.effects?.length || due <= s.dying.fired) return;
    s.dying.fired = due;
    const name = dying.effects[Math.floor(this.random() * dying.effects.length) % dying.effects.length];
    const bones = v.bones?.length ? v.bones : [v.root];
    const bone = bones[Math.floor(this.random() * bones.length) % bones.length] || v.root;
    bone.updateWorldMatrix(true, false);
    bone.matrixWorld.decompose(_position, _quaternion, _scale);
    this.effects.play(name, _position, null, null, _quaternion);
  }

  updateDust(v, s, fx, dt) {
    if (!s.dust || dt <= 0) return;
    s.dust.left -= dt;
    if (s.dust.left > 0) return;
    this.place(v, null, fx.deathDust.offset);
    this.effects.play(fx.deathDust.effect, _position, UP);
    s.dust = null;
  }
}
