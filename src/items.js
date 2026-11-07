import * as THREE from "three";
import { actorDescription } from "./actor-resources.js";
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

const POOLS = { powerupitem: 25, mine: 50 };
export class Items {
  // model(name) makes an item's model; explode(explosion, position, owner)
  // sets off an explosion; pools: the mission's pool sizes, by base.
  constructor({ scene, collision, effects, lights, model, size = () => null, explode, water = null, pools = {} }) {
    this.scene = scene;
    this.collision = collision;
    this.effects = effects;
    this.lights = lights;
    this.model = model;
    // size(name): the bounding sphere of a model's largest part, which sizes it without making one.
    this.size = size;
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
    // The answers the world's casts and water crossings are written into
    // (src/items-sim.js).
    this.hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3() };
    this.wet = new THREE.Vector3();
  }

  presentationMesh(def) {
    const make = def.GeometryName ? this.model(def.GeometryName) : null;
    const mesh = make ? make() : null;
    if (mesh) this.scene.add(mesh);
    return mesh;
  }

  // An item's model, made to show it (its mesh, null without one), with
  // its size and middle: the largest of its parts' bounding spheres.
  // What presentationModel measures, without making a mesh: whether the
  // definition has a model, and its size. A page online asks this once for
  // each kind, and makes each item's model when the item is first shown, as
  // the original made it.
  presentationSize(def) {
    const make = def.GeometryName ? this.model(def.GeometryName) : null;
    if (!make) return { model: false, radius: 0.3 };
    const sphere = this.size(def.GeometryName);
    return { model: true, radius: sphere ? sphere.radius : 0.3 };
  }

  presentationModel(def) {
    const mesh = this.presentationMesh(def);
    const radius = mesh ? radiusOf(mesh) : 0.3;
    return { mesh, radius, center: mesh?.userData.center ?? null };
  }

  clearPresentation() {
    for (const item of this.items) if (item.mesh) this.scene.remove(item.mesh);
    this.items.length = 0;
    this.views.clear();
    this.owned.clear();
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
}

// A model's size: the largest of its parts' bounding spheres, centre kept.
export function radiusOf(mesh) {
  let best = null;
  mesh.traverse((child) => {
    // modelPack's immutable bind sphere also sizes pickups and blasts. A
    // node's posed culling sphere is independent of that gameplay query.
    const asset = child.userData.actorAsset;
    const bounds = asset && actorDescription(asset).bounds;
    if (bounds && (!best || bounds[3] > best[3])) best = bounds;
  });
  if (!best) return 0.3;
  mesh.userData.center = new THREE.Vector3(best[0], best[1], best[2]);
  return best[3];
}
