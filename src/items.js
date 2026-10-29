import * as THREE from "three";
import { blastBlocked, blastDamage, blastDistance } from "./ordnance.js";
import { emitSound } from "./audio/events.js";

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
const PICKUP_DELAY = 0.5; // s a flying item can't be taken
const FLOOR = -500;
const MINE_SINK = 0.2; // m/s
const SOLDIER_RADIUS = 0.35; // m, the soldier's body for pickups
const SOLDIER_HEIGHT = 1.55; // m
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
    this.ownerIds = new WeakMap();
    this.nextOwnerId = 1;
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

  presentLight(item) {
    if (item.def.LightRadius > 0) this.lights.add(item.position, item.light ||= color01(item.def.LightColor), item.def.LightRadius);
  }

  clearPresentation() {
    for (const item of this.items) if (item.mesh) this.scene.remove(item.mesh);
    this.items.length = 0;
    this.views.clear();
    this.owned.clear();
  }

  // A new item, unless the world has no room for its kind. Returns it or null.
  spawn(def, position, velocity, yaw, { owner = null, team = 0 } = {}) {
    const live = this.items.filter((item) => item.def.base === def.base).length;
    if (live >= (this.pools[def.base] ?? Infinity)) return null;
    const mesh = this.presentationMesh(def);
    const item = {
      def,
      mine: def.base === "mine",
      owner,
      team,
      mesh,
      position: position.clone(),
      previous: position.clone(),
      velocity: velocity.clone(),
      yaw,
      up: UP.clone(),
      age: 0,
      life: def.LifeSpan ?? Infinity,
      health: def.MaxHealth ?? Infinity,
      landed: false,
      sinking: false,
      light: color01(def.LightColor),
      radius: mesh ? radiusOf(mesh) : 0.3,
    };
    this.place(item);
    this.items.push(item);
    return item;
  }

  // A dispenser's throw. An owner already with maxItems of this weapon's
  // items about loses the oldest first (a mine pops).
  ownerKey(owner) {
    if (!owner || typeof owner !== "object") return String(owner);
    if (!this.ownerIds.has(owner)) this.ownerIds.set(owner, this.nextOwnerId++);
    return `unit:${this.ownerIds.get(owner)}:${owner.generation ?? 0}`;
  }

  ownedBy(weapon, owner) {
    return this.owned.get(`${weapon}:${this.ownerKey(owner)}`) || [];
  }

  dispense(weapon, maxItems, def, position, velocity, yaw, owner, team) {
    const key = `${weapon}:${this.ownerKey(owner)}`;
    const list = (this.owned.get(key) || []).filter((item) => !item.gone);
    if (maxItems > 0 && list.length >= maxItems) this.destroy(list.shift());
    const item = this.spawn(def, position, velocity, yaw, { owner, team });
    if (item) list.push(item);
    this.owned.set(key, list);
    return item;
  }

  // A soldier's drop at death: one roll down the class's drop list.
  dropFor(drops, classes, position, velocity, yaw) {
    let r = (this.random || Math.random)();
    for (const drop of drops || []) {
      r -= drop.probability ?? 0;
      if (r >= 0) continue;
      const def = classes[drop.class];
      if (!def) return null;
      // Thrown out to the soldier's side at 3 m/s, unless he was moving
      // faster than that.
      const speed = Math.hypot(velocity.x, velocity.y, velocity.z);
      const out = speed > 3 ? velocity.clone() : new THREE.Vector3(Math.cos(yaw), 0, -Math.sin(yaw)).multiplyScalar(3);
      return this.spawn(def, position, out, yaw);
    }
    return null;
  }

  // Gone without a blast: a mine pops (ExplosionDeath), a power-up vanishes.
  destroy(item) {
    if (item.gone) return;
    // Gone before its blast: that blast reaches the mines beside it, whose
    // own blasts reach back here, and a mine that is already gone ends the
    // chain instead of the two setting each other off without end.
    this.remove(item);
    if (item.mine && item.def.ExplosionDeath && typeof item.def.ExplosionDeath === "object") {
      this.explode(item.def.ExplosionDeath, item.position.clone(), item.owner);
    }
  }

  remove(item) {
    item.gone = true;
    if (item.mesh) this.scene.remove(item.mesh);
  }

  // The end of an item's LifeSpan: a mine with an ExplosionExpire (the time
  // bomb) sets it off with its thrower's credit, gone before its blast as in
  // destroy(); anything else is just gone.
  expire(item) {
    if (item.gone) return;
    this.remove(item);
    if (item.mine && typeof item.def.ExplosionExpire === "object") this.explode(item.def.ExplosionExpire, item.position.clone(), item.owner);
  }

  // Damage to one item (a mine at 0 health pops).
  hurt(item, amount) {
    if (item.gone || !item.mine || !Number.isFinite(item.health)) return;
    item.health -= amount;
    if (item.health <= 0) this.destroy(item);
  }

  // A blast: damage to the mines lying about (src/ordnance.js), as a person
  // unless the class says otherwise; one still in flight is out of its
  // reach.
  blast(explosion, position) {
    for (const item of this.items) {
      if (item.gone || !item.mine || !item.landed || !Number.isFinite(item.health)) continue;
      const damage = blastDamage(explosion, blastDistance(position, item.position, item.radius), item.def.HealthType);
      if (!(damage > 0) || blastBlocked(this.collision, position, item.position)) continue;
      item.health -= damage;
      if (item.health <= 0) this.destroy(item);
    }
  }

  // targets: soldiers who can take power-ups or set off mines, each with
  // position, velocity, team, relation(team) (below 0 an enemy, above 0 a
  // friend), healthType, and give(def) -> whether anything was taken.
  update(dt, targets = []) {
    for (const item of this.items) {
      if (item.gone) continue;
      item.age += dt;
      item.life -= dt;
      if (item.life <= 0 || item.position.y < FLOOR) {
        // A time bomb's timer ran out: it goes off where it lies.
        if (item.life <= 0) this.expire(item);
        else this.remove(item);
        continue;
      }
      if (!item.landed) this.fly(item, dt);
      if (item.mine) this.watch(item, targets);
      else this.offer(item, targets);
      if (item.gone) continue;
      this.presentLight(item);
    }
    this.items = this.items.filter((item) => !item.gone);
    for (const [key, list] of this.owned) {
      const remaining = list.filter((item) => !item.gone);
      if (remaining.length) this.owned.set(key, remaining);
      else this.owned.delete(key);
    }
  }

  fly(item, dt) {
    const g = -GRAVITY * (item.def.Gravity ?? 1);
    const v = item.velocity;
    if (item.sinking) v.set(0, -MINE_SINK, 0);
    else v.y += g * 0.5 * dt;
    item.previous.copy(item.position);
    item.position.addScaledVector(v, dt);
    if (!item.sinking) v.y += g * 0.5 * dt;
    if (item.mine && !item.sinking && this.water?.crossing(item.previous, item.position, this.wet)) {
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
    const hit = this.collision.cast(this.start, ray, length + 0.1, this.hit);
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

  // The model at the item, upright on its surface, facing its way.
  place(item) {
    if (!item.mesh) return;
    item.mesh.position.copy(item.position);
    const up = item.up;
    const forward = this.forward.set(Math.sin(item.yaw), 0, Math.cos(item.yaw));
    forward.addScaledVector(up, -forward.dot(up));
    if (forward.lengthSq() < 1e-8) forward.set(0, 0, 1).addScaledVector(up, -up.z);
    forward.normalize();
    const right = this.right.crossVectors(up, forward).normalize();
    item.mesh.quaternion.setFromRotationMatrix(this.basis.makeBasis(right, up, forward));
  }

  // A mine: set off by an enemy near enough and no longer closing in, unless
  // a friend is near.
  watch(item, targets) {
    const def = item.def;
    if (!item.landed || !def.ExplosionTrigger || !(def.TriggerRadius > 0)) return;
    let fire = false;
    for (const target of targets) {
      const relation = target.relation(item.team);
      if (relation === 0) continue;
      const toward = this.ray.subVectors(target.center, item.position);
      const gap = toward.lengthSq();
      if (relation > 0) {
        if (def.SuppressRadius > 0 && gap < (def.SuppressRadius + target.radius) ** 2) return;
        continue;
      }
      if (gap < (def.TriggerRadius + target.radius) ** 2 && target.velocity.dot(toward) >= 0) fire = true;
    }
    if (!fire) return;
    if (typeof def.ExplosionTrigger === "object") this.explode(def.ExplosionTrigger, item.position.clone(), item.owner);
    this.remove(item);
  }

  // A power-up: taken by the first soldier touching it that gets something.
  offer(item, targets) {
    if (!item.landed && item.age <= PICKUP_DELAY) return;
    const c = this.start.copy(item.mesh?.userData.center || ORIGIN);
    if (item.mesh) c.applyQuaternion(item.mesh.quaternion);
    c.add(item.position);
    for (const target of targets) {
      const p = target.position;
      const across = Math.hypot(c.x - p.x, c.z - p.z);
      if (across > SOLDIER_RADIUS + item.radius) continue;
      if (c.y < p.y - item.radius || c.y > p.y + SOLDIER_HEIGHT + item.radius) continue;
      if (target.give(item.def)) {
        emitSound("pickup", item.def, item.position);
        this.remove(item);
        return;
      }
    }
  }
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
    soldier.health = Math.min(soldier.health + health, soldier.maxHealth);
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
