import * as THREE from "three";
import { ItemsWorld } from "./ordnance-world.js";

// Things lying about: mines and power-ups, thrown by dispensers or dropped by
// the dead, following the 2004 game (EntityItem, EntityMine,
// PowerupItem).
//
// Flight: gravity (Gravity x 9.8) in two half steps around the move. Power-ups
// meeting the world at 1 m/s or more into it lose that part, then Friction x
// it x (1 + Rebound) of the rest, and bounce back at Rebound of it; slower
// than that they settle, upright on what they touch. Soldiers pass through
// them. Mines stick to the first thing that isn't a soldier, upright on it,
// and bounce off soldiers. In water a mine sinks at 0.2 m/s.
//
// Power-ups last LifeSpan (30 s), and anyone touching one once it has landed
// (or after its first 0.5 s) takes it, whoever threw it: health (by the
// receiver's HealthType, up to its MaxHealth, not for a unit whose AddHealth
// is below 0) and clips (to every weapon but a power-up dispenser, as many as
// it carries). One that gives nothing stays. Taking health plays the
// soldier's HealthGainEffect, ammo its AmmoGainEffect.
//
// Mines last until something sets them off. An enemy within TriggerRadius
// (plus his size) that isn't closing in any more (still, moving away, or
// passing) sets one off (ExplosionTrigger, with its owner's credit), unless a
// friend is within SuppressRadius; teammates and the neutral never do.
// Blasts damage them (they take damage as a person), and at 0 health they
// pop harmlessly (ExplosionDeath). A steady light (LightColor, LightRadius)
// marks them. A time bomb is a mine with a LifeSpan and no TriggerRadius:
// when its time runs out it goes off (ExplosionExpire, with its thrower's
// credit).
//
// A dispenser's owner keeps at most MaxItems of its items about: throwing
// another removes the oldest (a mine pops). The world holds at most 25
// power-ups and 50 mines (a mission's SetMemoryPoolSize says otherwise);
// past that, nothing is thrown and nothing is spent.
//
// The port's own: where a stuck mine sits (on the surface, not half in it),
// and power-ups vanish at the end of their life where the original fades them
// over a second.

const GRAVITY = 9.8;
const HARD = 1; // m/s into a surface that bounces rather than settles
export const MINE_SINK = 0.2; // m/s
const POOLS = { powerupitem: 25, mine: 50 };
const AMOUNTS = {
  person: ["PersonHealth", "PersonAmmo"],
  animal: ["AnimalHealth", "AnimalAmmo"],
  droid: ["DroidHealth", "DroidAmmo"],
  vehicle: ["VehicleHealth", "VehicleAmmo"],
  building: ["BuildingHealth", "BuildingAmmo"],
};
const UP = new THREE.Vector3(0, 1, 0);

const color01 = (rgba = [0, 0, 0, 0]) => {
  const a = (rgba[3] ?? 255) / 255;
  return [(rgba[0] / 255) * a, (rgba[1] / 255) * a, (rgba[2] / 255) * a];
};

export class Items {
  // model(name) makes an item's model; explode(explosion, position, owner)
  // sets off an explosion; pools: the mission's pool sizes, by base.
  constructor({ scene, collision, effects, lights, model, explode, water = null, pools = {} }) {
    this.scene = scene;
    this.collision = collision;
    this.effects = effects;
    this.lights = lights;
    this.model = model;
    this.explode = explode;
    this.water = water;
    this.pools = { ...POOLS, ...pools };
    this.items = [];
    this.views = new Set(); // items a client online shows (network-entity-items.js), for their sounds
    this.owned = new Map(); // dispenser class + owner -> its items, oldest first
    // Everything the items ask of the rest of the battle
    // (src/ordnance-world.js).
    this.world = new ItemsWorld(this);
    // The items on the simulation's module (src/items-sim.js), once a world
    // hands them over: update and the commands below run there and the
    // lists hold mirrors of what it holds.
    this.sim = null;
    this.hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3() };
    this.ray = new THREE.Vector3();
    this.start = new THREE.Vector3();
    this.wet = new THREE.Vector3();
    this.forward = new THREE.Vector3();
    this.right = new THREE.Vector3();
    this.basis = new THREE.Matrix4();
  }

  presentationMesh(def) {
    const make = def.GeometryName ? this.model(def.GeometryName) : null;
    const mesh = make ? make() : null;
    if (mesh) this.scene.add(mesh);
    return mesh;
  }

  // An item's model, made to show it (its mesh, null without one), with
  // its size and middle: the largest of its parts' bounding spheres.
  presentationModel(def) {
    const mesh = this.presentationMesh(def);
    const radius = mesh ? radiusOf(mesh) : 0.3;
    return { mesh, radius, center: mesh?.userData.center ?? null };
  }

  presentLight(item) {
    if (item.def.LightRadius > 0) this.lights.add(item.position, item.light ||= color01(item.def.LightColor), item.def.LightRadius);
  }

  clearPresentation() {
    for (const item of this.items) if (item.mesh) this.scene.remove(item.mesh);
    this.items.length = 0;
    this.views.clear();
    this.owned.clear();
  }

  // What fly() steps and place() draws: the world's items (spawn), and the
  // copies a page online flies between their records
  // (src/runtime/network-entity-items.js). The radius is the model's, so a
  // copy sticks a mine where the world does; the model's middle, turned as
  // the model is (`turn`, place()), is where a power-up is taken from.
  itemState(def, mesh, position, velocity, yaw, radius = mesh ? radiusOf(mesh) : 0.3, center = mesh?.userData.center ?? null) {
    return {
      def,
      mine: def.base === "mine",
      mesh,
      position: position.clone(),
      previous: position.clone(),
      velocity: velocity.clone(),
      yaw,
      up: UP.clone(),
      landed: false,
      sinking: false,
      light: color01(def.LightColor),
      radius,
      center,
      turn: mesh ? new THREE.Quaternion() : null,
    };
  }

  // A new item, unless the world has no room for its kind. Returns it or null.
  spawn(def, position, velocity, yaw, { owner = null, team = 0 } = {}) {
    return this.sim.spawn(def, position, velocity, yaw, { owner, team });
  }

  // A dispenser's throw. An owner already with maxItems of this weapon's
  // items about loses the oldest first (a mine pops).
  ownerKey(owner) {
    return this.world.ownerKey(owner);
  }

  ownedBy(weapon, owner) {
    return this.owned.get(`${weapon}:${this.ownerKey(owner)}`) || [];
  }

  dispense(weapon, maxItems, def, position, velocity, yaw, owner, team) {
    return this.sim.dispense(weapon, maxItems, def, position, velocity, yaw, owner, team);
  }

  // A soldier's drop at death: one roll down the class's drop list.
  dropFor(drops, classes, position, velocity, yaw) {
    return this.sim.dropFor(drops, classes, position, velocity, yaw);
  }

  // The end of an item's LifeSpan: a mine with an ExplosionExpire (the time
  // Damage to one item (a mine at 0 health pops).
  hurt(item, amount) {
    return this.sim.hurt(item, amount);
  }

  // A blast: damage to the mines lying about (src/ordnance.js), as a person
  // unless the class says otherwise; one still in flight is out of its
  // reach.
  blast(explosion, position) {
    return this.sim.blast(explosion, position);
  }

  // targets: soldiers who can take power-ups or set off mines, each with
  // position, velocity, team, relation(team) (below 0 an enemy, above 0 a
  // friend), healthType, and give(def) -> whether anything was taken.
  update(dt, targets = []) {
    return this.sim.update(dt, targets);
  }

  fly(item, dt) {
    const g = -GRAVITY * (item.def.Gravity ?? 1);
    const v = item.velocity;
    if (item.sinking) v.set(0, -MINE_SINK, 0);
    else v.y += g * 0.5 * dt;
    item.previous.copy(item.position);
    item.position.addScaledVector(v, dt);
    if (!item.sinking) v.y += g * 0.5 * dt;
    if (item.mine && !item.sinking && this.world.waterCrossing(item.previous, item.position, this.wet)) {
      item.sinking = true;
      v.set(0, -MINE_SINK, 0);
      this.water.ripple(this.wet, 1.5, 0.01);
    }
    this.contact(item);
    this.place(item);
  }

  // What the item meets between where it was and where it is.
  contact(item) {
    const ray = this.ray.subVectors(item.position, item.previous);
    const length = ray.length();
    if (length < 1e-6) return;
    ray.divideScalar(length);
    this.start.copy(item.previous).addScaledVector(ray, -0.05);
    const hit = this.world.cast(this.start, ray, length + 0.1, this.hit);
    if (!hit) return;
    const n = hit.normal;
    const v = item.velocity;
    const d = v.dot(n);
    if (d >= 0) return;
    if (item.mine) {
      // Stuck, upright on the surface (resting on it, the port's choice).
      item.position.copy(hit.point).addScaledVector(n, item.radius * 0.3);
      this.settle(item, n);
      return;
    }
    if (d > -HARD) {
      item.position.copy(hit.point);
      this.settle(item, n);
      return;
    }
    item.position.copy(hit.point).addScaledVector(n, 0.02);
    const rebound = item.def.Rebound ?? 0.3;
    v.addScaledVector(n, -d);
    const along = v.length();
    if (along > 1e-6) v.multiplyScalar(1 - Math.min(((item.def.Friction ?? 0.5) * -d * (1 + rebound)) / along, 1));
    v.addScaledVector(n, rebound * -d);
  }

  settle(item, normal) {
    item.landed = true;
    item.sinking = false;
    item.velocity.set(0, 0, 0);
    item.up.copy(normal);
  }

  // The model at the item, upright on its surface, facing its way (its
  // turn), and drawn there.
  place(item) {
    if (!item.turn) return;
    const up = item.up;
    const forward = this.forward.set(Math.sin(item.yaw), 0, Math.cos(item.yaw));
    forward.addScaledVector(up, -forward.dot(up));
    if (forward.lengthSq() < 1e-8) forward.set(0, 0, 1).addScaledVector(up, -up.z);
    forward.normalize();
    const right = this.right.crossVectors(up, forward).normalize();
    item.turn.setFromRotationMatrix(this.basis.makeBasis(right, up, forward));
    if (item.mesh) {
      item.mesh.position.copy(item.position);
      item.mesh.quaternion.copy(item.turn);
    }
  }

  // A power-up: taken by the first soldier touching it that gets something.

}

const ORIGIN = new THREE.Vector3();

// A model's size: the largest of its parts' bounding spheres, centre kept.
function radiusOf(mesh) {
  let best = null;
  mesh.traverse((child) => {
    const sphere = child.geometry?.boundingSphere;
    if (sphere && (!best || sphere.radius > best.radius)) best = sphere;
  });
  if (!best) return 0.3;
  mesh.userData.center = best.center.clone();
  return best.radius;
}

// What a power-up gives a soldier: health and clips by its health type.
// soldier: { health, maxHealth, addHealth, weapons, healthType }. Returns
// what was given: { health, ammo } (booleans).
export function givePowerup(def, soldier) {
  const [healthKey, ammoKey] = AMOUNTS[soldier.healthType] || AMOUNTS.person;
  const out = { health: false, ammo: false };
  const health = def[healthKey] || 0;
  if (health && soldier.health < soldier.maxHealth && !(soldier.addHealth < 0)) {
    if (soldier.system?.f2) soldier.setHealth(Math.min(soldier.health + health, soldier.maxHealth));
    else soldier.health = Math.min(soldier.health + health, soldier.maxHealth);
    out.health = true;
  }
  const ammo = def[ammoKey] || 0;
  if (ammo) {
    for (const weapon of soldier.weapons) {
      // A power-up dispenser takes no power-ups.
      if (weapon.def.base === "dispenser" && weapon.def.item?.base === "powerupitem") continue;
      if (weapon.bottomless || !(weapon.def.ammo > 0)) continue;
      const loaded = weapon.clip >= weapon.perShot - 1e-6 ? 1 : 0;
      const next = THREE.MathUtils.clamp(weapon.reserve + ammo, 0, weapon.def.ammo - loaded);
      if (Math.abs(next - weapon.reserve) > 1e-4) {
        weapon.reserve = next;
        out.ammo = true;
      }
    }
  }
  return out;
}
