import * as THREE from "three";
import { buildRegion, inRegion } from "./regions.js";

// Destructible command posts, the rest of what the 2004 game does
// with them (docs/objectives.md): the building is src/props.js
// ObjectivePost, the post src/commandposts.js.
//
// - A target: the 2004 game's target check takes any living
//   object with a finite MaxHealth whose owner the viewer is hostile to,
//   and the post entity answers yes. So a destructible post
//   joins the units registry as a building (classifier 9, category 4), for
//   the bots' sight and weapons and the player's reticle and enemy health
//   bar, while it stands.
// - Its KillRegion, once, as it is destroyed: 15 explosion, med_explosion
//   or small_explosion effects at points drawn through the region's box
//   (each axis uniform over its extent), then every soldier and vehicle
//   whose origin is inside dies, no one credited. Hoth's shield generator
//   has one (CP2_Death).

export const KILL_REGION_BLASTS = 15;
const BLASTS = ["explosion", "med_explosion", "small_explosion"];

// The units registry's entry for a destructible post's building.
export class ObjectiveTarget {
  constructor(building) {
    this.building = building;
    this.id = 0;
    this.generation = 1;
    this.kind = 9; // a building
    this.category = 4;
    this.className = building.desc.class || building.desc.model || "commandpost";
    this.classDef = building.desc.settings || {};
    this.human = false;
    this.hero = false;
    this.walker = false;
    this.character = null;
    this.controller = null;
    this.seat = null;
    this.bounds = { center: new THREE.Vector3(), radius: 1 };
    const target = this;
    this.combat = {
      get health() { return target.building.health; },
      get maxHealth() { return target.building.maxHealth; },
      get dead() { return !target.building.alive; },
      get team() { return target.team; },
      explode() {},
    };
  }

  get alive() { return this.building.alive; }
  get dead() { return !this.building.alive; }
  get team() { return this.building.team ?? 0; }
  get health() { return this.building.health; }
  get maxHealth() { return this.building.maxHealth; }
  get position() { return this.building.position; }
  get velocity() { return ZERO; }

  get sphere() {
    this.bounds.radius = this.building.sphere(this.bounds.center);
    return this.bounds;
  }

  aimPoint(out) {
    this.building.sphere(out);
    return out;
  }

  origin(out) { return this.aimPoint(out); }
  forward(out) { return out.set(0, 0, 1).transformDirection(this.building.matrix); }
}
const ZERO = new THREE.Vector3();

// Every destructible post's building in the units registry.
export function registerObjectives(live, units) {
  for (const building of live?.objectives || []) {
    building.aiTarget ||= new ObjectiveTarget(building);
    if (units.byId(building.aiTarget.id) !== building.aiTarget) units.add(building.aiTarget);
  }
}

// A post's KillRegion ({ type, matrix, size }) as it goes: the effects'
// points (random() draws), then whoever stands inside it dies.
// play(name, point): an effect; victims(): the soldiers and vehicles to
// test, each { position, kill() }.
export function killRegion(region, { random = Math.random, play, victims }) {
  if (!region) return 0;
  const built = buildRegion(region);
  const matrix = built.inverse.clone().invert();
  const [a, b, c] = built.half;
  const point = new THREE.Vector3();
  for (let i = 0; i < KILL_REGION_BLASTS; i++) {
    point.set((random() * 2 - 1) * a, (random() * 2 - 1) * b, (random() * 2 - 1) * c).applyMatrix4(matrix);
    play?.(BLASTS[Math.floor(random() * 3) % 3], point.clone());
  }
  let killed = 0;
  for (const victim of victims?.() || []) {
    if (killed >= 64) break;
    if (!inRegion(built, victim.position, point)) continue;
    victim.kill();
    killed += 1;
  }
  return killed;
}
