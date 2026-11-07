// Immutable hull resources and collision identities shared by native owners.
// F2 steps a canonical fleet; the page only constructs private predictions.
// Loading these facts never creates a page authority clock or seat lifecycle.
import { F2ColliderHeaderLayout as COLLIDER, putF2Bodies } from '../gen/f2.js';
import { KIND_BODY } from '../gen/index.js';
import { json } from './vehicle-phase-data.js';
export class VehicleContent {
  constructor(vehicles, bridge) {
    this.vehicles = vehicles; this.bridge = bridge; this.x = bridge.x;
    this.collision = vehicles.collision; this.kernel = false;
    this.bodiesVersion = -1; this.colliders = new WeakSet();
    // Body identity is shared by raycasts and native hull queries. Retained
    // argument storage makes later topology updates independent of replay.
    this.bodyArgs = { words: this.x.f2_body_words(), put: putF2Bodies };
    this.bodyRefs = { bodies: [] };
  }
  // Flight tuning keys resolve only at content load, before pending replay.
  retune(name, key, value) {
    if (typeof value === "number") this.x.veh_flight_tune(this.bridge.strings.id(name), this.bridge.strings.id(key), value);
  }

  // Vehicle construction supplies the immutable model, collision and clips.
  // Rust keeps this pack; ordinary page frames never serialize its geometry.
  pack({ data, buffer, indexBuffer, animationBuffer }) {
    // Keep JSON negative zero through the existing content encoder; changing
    // those signs would change poses before prediction receives a checkpoint.
    const text = new TextEncoder().encode(json({ models: data.models, collision: data.collision, animations: data.animations, classes: data.classes }));
    const head = Math.ceil(text.length / 4) * 4, floats = buffer?.byteLength ?? 0, indices = indexBuffer?.byteLength ?? 0, clips = animationBuffer?.byteLength ?? 0;
    const ptr = this.x.veh_bytes(head + floats + indices + clips);
    this.bridge.views();
    const u8 = this.bridge.u8;
    u8.set(text, ptr);
    if (floats) u8.set(new Uint8Array(buffer), ptr + head);
    if (indices) u8.set(new Uint8Array(indexBuffer), ptr + head + floats);
    if (clips) u8.set(new Uint8Array(animationBuffer), ptr + head + floats + indices);
    return this.x.veh_pack(text.length, floats / 4, indices / 4, clips / 4) > 0;
  }

  // Cold registrations share the module exchange buffer. Retain no view
  // across a call that can grow its memory.
  input(count) {
    const ptr = this.x.veh_input(count);
    this.bridge.views();
    return ptr / 8;
  }

  // Name the shared collision kernel once, then refresh only new body handles.
  // Each prediction owner copies mappings but retains separate enabled flags.
  // Late replica births can add a collider without importing a fleet snapshot.
  bodiesIn() {
    const c = this.collision, rays = c?.rays;
    if (!rays) throw Error('The vehicle phase requires the native collision kernel');
    if (!this.kernel) {
      const blockers = c.vehicleBlockers ? rays.fixedSet(c.blockerSet(c.vehicleBlockers), c.vehicleBlockers.bvh) : -1;
      this.x.veh_collision(c.rayWorld, blockers);
      this.kernel = true;
      this.bodiesVersion = -1;
    }
    c.rayBodies();
    const bodies = c.bodies;
    if (this.bodiesVersion !== c.bodiesVersion) {
      const rows = this.bodyRefs.bodies; rows.length = bodies.length;
      for (let i = 0; i < bodies.length; i++) { const row = rows[i] ??= { body: null, kernel: 0 }; row.body = bodies[i]; row.kernel = bodies[i].raySlot; }
      this.bridge.callArgs(this.bodyArgs, this.bodyRefs); this.x.f2_bodies();
      this.bodiesVersion = c.bodiesVersion;
    }
    for (const v of this.vehicles.vehicles) if (!this.colliders.has(v.body)) this.collider(v);
    this.flags = this.x.veh_flags();
  }

  // Wall and spring caches are constructed with the collider. Their immutable
  // handles are reused by every pending tick in the private native session.
  // This cold path can grow memory; a warm replay cannot call it per tick.
  collider(v) {
    const rays = this.collision.rays, body = v.body;
    // (Making a cache's copy may grow the memory: the ids first.)
    const walls = (v.wallCaches ?? []).map((cache) => (cache ? rays.cache(cache) : -1));
    const springs = (v.springCaches ?? []).map((cache) => (cache ? rays.cache(cache) : -1));
    const slot = this.bridge.refs[KIND_BODY].slotOf(body);
    const at = this.input(COLLIDER.words + walls.length + springs.length), f = this.bridge.f, b = body.localBounds;
    f[at + COLLIDER.body] = slot; f[at + COLLIDER.kernel] = body.raySlot;
    f[at + COLLIDER.min] = b.min.x; f[at + COLLIDER.min + 1] = b.min.y; f[at + COLLIDER.min + 2] = b.min.z;
    f[at + COLLIDER.max] = b.max.x; f[at + COLLIDER.max + 1] = b.max.y; f[at + COLLIDER.max + 2] = b.max.z;
    for (let i = 0; i < walls.length; i++) f[at + COLLIDER.words + i] = walls[i];
    for (let i = 0; i < springs.length; i++) f[at + COLLIDER.words + walls.length + i] = springs[i];
    this.x.veh_collider(walls.length, springs.length);
    this.colliders.add(body);
  }
}
