import * as THREE from "three";

// Command vehicles as command posts, as the 2004 game makes them
// (docs/human-spawn.md "Command vehicles"; formats.md "Spawning and the
// spawn screen").
//
// A vehicle of the native command classes (ODF base commandwalker: the AT-AT
// and the AT-TE; commandhover: the MTT and the hovernaut) is a command post
// while it lives. Its constructor takes a post from the global list (the
// CommandWalker and CommandHover constructors): the dead post of the same
// class within 1 m of where it was made, else a new one after the placed
// posts.
// So one post serves a vehicle pad for the whole battle, which the port keeps
// as one post per pad, named by the pad's slot (transport-<slot>, the names the
// network registry reserves, src/runtime/entity-spawn-posts.js).
//
// - Live: the post runs from the vehicle's activation to its death (the
//   walker update's death branch calls its OnDeath, which deactivates the
//   post), so a dying walker is no post.
// - Owner: the post's owner is its vehicle's team. Taking a seat gives the
//   hull the occupant's team, and a command class keeps it when the hull
//   empties (its class constructors clear the revert-to-neutral flag), so a
//   hull an enemy took stays his.
// - Where: the post is where its vehicle is now (its entity transform), for
//   the spawn screen's nearest-post rules and the AI's objective
//   destination.
// - Spawning: a team spawns at its own command vehicle at one of the class's
//   spawn points (SpawnPointCount, then SpawnPointLocation "x y z yawDeg"),
//   drawn uniformly, in the vehicle's frame: the point and the yaw
//   turned and moved with the hull, then pulled back toward the hull's
//   collision centre to the first terrain, building or prop between them
//   (mask 0x48, the hull itself left out). The soldier settles from there.
// - Counting: its ValueBleed counts for its owner's bleed, and holding it
//   resets the no-post count (the Team update iterates the same list); its
//   Value_ATK/DEF weights make it an objective. It has no capture region, so
//   it never changes hands by capture.
// - Lost: when it dies its post leaves the list. The bots sent there draw
//   again, "<team> lost <post>" is announced (event 5), and whoever hurt it
//   last earns a command post point unless he is its owner's friend.

const COMMAND_BASES = new Set(["commandwalker", "commandhover"]);
const DEFAULT_VALUES = () => [100, 100, 100, 100, 100, 100];
const DEG = Math.PI / 180;

export function isCommandClass(def) {
  return COMMAND_BASES.has(def?.base);
}

// A class's post data: ai-mission.json `commandClasses` (the original game's
// class parser: weights, ValueBleed, spawn points), else what the vehicle
// class itself says (its ValueBleed, the class defaults, no spawn points).
function classData(def, record) {
  if (record) {
    return {
      atk: record.values?.atk?.slice() ?? DEFAULT_VALUES(),
      def: record.values?.def?.slice() ?? DEFAULT_VALUES(),
      valueBleed: record.valueBleed ?? 100,
      points: (record.spawnPoints || []).map((p) => [p[0] ?? 0, p[1] ?? 0, p[2] ?? 0, p[3] ?? 0]),
    };
  }
  const bleed = Number.parseInt(def?.settings?.ValueBleed, 10);
  return { atk: DEFAULT_VALUES(), def: DEFAULT_VALUES(), valueBleed: Number.isFinite(bleed) ? bleed : 100, points: [] };
}

const scratch = { local: new THREE.Vector3(), from: new THREE.Vector3(), direction: new THREE.Vector3(), hit: { point: new THREE.Vector3(), normal: new THREE.Vector3() } };

// The world point of a class spawn point on a vehicle now: [x, y, z, yaw].
export function commandSpawnPoint(vehicle, point, collision = vehicle?.system?.collision) {
  const { local, from, direction, hit } = scratch;
  local.set(point[0], point[1], point[2]).applyMatrix4(vehicle.matrix);
  const yaw = vehicle.yaw + (point[3] ?? 0) * DEG;
  // Back toward the hull's collision centre to the first static surface.
  from.copy(vehicle.center).applyMatrix4(vehicle.matrix);
  direction.subVectors(local, from);
  const span = direction.length();
  if (collision && span > 1e-6) {
    direction.divideScalar(span);
    if (collision.cast(from, direction, span, hit, vehicle.body, true)) local.copy(hit.point);
  }
  return [local.x, local.y, local.z, yaw];
}

// One vehicle pad's post.
export class MobilePost {
  constructor(system, slot, index, classes) {
    this.system = system;
    this.slot = slot;
    this.index = index;
    this.classes = classes || {};
    this.name = `transport-${index}`;
    this.label = "";
    this.team = 0;
    this.active = false;
    this.mobile = true;
    this.vehicle = null; // the hull of its current or last life
    this.life = null; // that hull's registry generation
    this.yaw = 0;
    this.radius = 0; // the hull's collision sphere (the AI's arrival test)
    this.speed = 0; // its nominal forward speed: a post that moves
    this.lastPosition = new THREE.Vector3();
    this.desc = { name: "", displayName: "", valueBleed: 100, mobile: true };
    const post = this;
    this.ai = {
      order: null,
      atk: DEFAULT_VALUES(),
      def: DEFAULT_VALUES(),
      bans: 0,
      valueBleed: 100,
      spawnPath: null,
      allyPath: null,
      allyCount: 0,
      classSpawnPoints: 0,
      points: [],
      // The class's spawn points where the hull is now.
      get classSpawnWorld() {
        const v = post.vehicle;
        return v && post.active ? this.points.map((p) => commandSpawnPoint(v, p)) : [];
      },
      capture: null,
      radius: 0,
      get yaw() {
        return post.yaw;
      },
    };
  }

  get position() {
    return this.vehicle && this.active ? this.vehicle.position : this.lastPosition;
  }

  // The pad's live command vehicle, if any.
  hull() {
    let found = null;
    for (const v of this.slot.variants?.values?.() ?? []) if (v.alive && isCommandClass(v.def)) found = v;
    if (!found && this.slot.vehicle?.alive && isCommandClass(this.slot.vehicle.def)) found = this.slot.vehicle;
    return found;
  }

  refresh() {
    const v = this.hull();
    if (!v) {
      if (this.vehicle) this.lastPosition.copy(this.vehicle.position);
      this.active = false;
      return this;
    }
    const life = v.aiTarget?.generation ?? 0;
    if (v !== this.vehicle || life !== this.life) {
      // A new hull on the pad: its class's post data, and its spawn team.
      this.vehicle = v;
      this.life = life;
      this.team = v.team | 0;
      const data = classData(v.def, this.classes[v.name]);
      Object.assign(this.ai, { atk: data.atk, def: data.def, valueBleed: data.valueBleed, points: data.points, classSpawnPoints: data.points.length });
      this.label = v.label || v.name;
      this.desc = { name: v.name, displayName: this.label, valueBleed: data.valueBleed, mobile: true };
      this.radius = v.size ? v.size.length() / 2 : v.radius ?? 0;
      this.speed = v.speed ?? 0;
    }
    // Whoever takes a seat makes it his team's; it stays his when he leaves.
    if (v.seats?.some((seat) => seat.lease && !seat.lease.released)) this.team = v.controllingTeam | 0;
    this.yaw = v.yaw;
    this.active = true;
    return this;
  }
}

// Every pad whose classes include a command vehicle, in pad order.
export class MobilePosts {
  constructor(system, classes = {}) {
    this.system = system;
    this.posts = [];
    if (!system?.slots) return;
    system.slots.forEach((slot, index) => {
      const names = Object.values(slot.desc?.classes || {});
      if (names.some((name) => isCommandClass(system.data?.classes?.[name]))) this.posts.push(new MobilePost(system, slot, index, classes));
    });
    for (const post of this.posts) post.reported = this.state(post.refresh());
  }

  state(post) {
    return { active: post.active, team: post.team, vehicle: post.vehicle, life: post.life };
  }

  refresh() {
    for (const post of this.posts) post.refresh();
    return this.posts;
  }

  // What changed since the last call, for the battle's authority: posts
  // lost (their vehicle died) and posts that changed hands.
  //   { kind: "lost", post, vehicle, from }   { kind: "owner", post, from, to }
  events() {
    const out = [];
    for (const post of this.posts) {
      const was = post.reported;
      post.refresh();
      const replaced = post.vehicle !== was.vehicle || post.life !== was.life;
      if (was.active && (!post.active || replaced)) out.push({ kind: "lost", post, vehicle: was.vehicle, from: was.team });
      else if (was.active && post.active && was.team !== post.team) out.push({ kind: "owner", post, from: was.team, to: post.team });
      post.reported = this.state(post);
    }
    return out;
  }
}
