import {sceneProperty,sceneTransform} from "./rust-render/scene-changes.js";
import * as THREE from "three";
import { MovementKernel } from "./sim/wasm/movement-kernel.js";
import { CastRay as CR } from "./presentation/layout.js";


// Polygon numbers of the vehicle-only barriers, apart from the buildings'
// and the bodies'.
const BLOCKER_POLYGONS = 0x70000000;
// A gather finding more polygons than this files them in a PolygonIndex
// (polygon()).
const GROUP_SCAN = 16;
// The ray kernel's answers for no hit and a terrain hit (MISS and TERRAIN
// in rust/sim/src/collision); a body's slot is 0 or more, and -1 the
// buildings.
const RAY_MISS = -2, RAY_TERRAIN = -3;

// The current gather's polygons past GROUP_SCAN of them, by polygon number:
// open addressing over typed arrays, emptied by moving on to the next stamp,
// so a gather makes nothing. A Map did this, and once a full collection had
// moved it to the old generation, every table it grew or was cleared to was
// made there too: several a tick, growing to 64 entries, garbage only the
// next full collection takes.
class PolygonIndex {
  constructor(bits = 8) { this.stamp = 1; this.allocate(bits); }
  allocate(bits) {
    const size = 1 << bits;
    this.bits = bits; this.mask = size - 1; this.count = 0;
    this.ids = new Float64Array(size); this.at = new Int32Array(size); this.stamps = new Uint32Array(size);
  }
  clear() {
    this.count = 0;
    if (++this.stamp > 0xffffffff) { this.stamps.fill(0); this.stamp = 1; }
  }
  // The slot holding `id`, or the free slot it would take.
  slot(id) {
    let h = Math.imul(id >>> 0, 0x9e3779b1) >>> (32 - this.bits);
    while (this.stamps[h] === this.stamp && this.ids[h] !== id) h = (h + 1) & this.mask;
    return h;
  }
  // The list position filed under `id`, or -1.
  get(id) {
    const h = this.slot(id);
    return this.stamps[h] === this.stamp ? this.at[h] : -1;
  }
  set(id, at) {
    if (2 * (this.count + 1) > this.mask + 1) this.grow();
    const h = this.slot(id);
    if (this.stamps[h] !== this.stamp) { this.stamps[h] = this.stamp; this.ids[h] = id; this.count++; }
    this.at[h] = at;
  }
  grow() {
    const { ids, at, stamps, stamp } = this;
    this.allocate(this.bits + 1);
    for (let i = 0; i < ids.length; i++) if (stamps[i] === stamp) this.set(ids[i], at[i]);
  }
}

// Empties a list kept for reuse. Setting its length to 0 is a runtime call
// even on an empty list, and drops its storage for the next push to
// allocate again; popping keeps it.
function empty(list) {
  while (list.length) list.pop();
}

// The map's collision, laid out the way the engine tests a soldier against it.
//
// Buildings and props are polygons. Each triangle knows the polygon it came
// from, because a soldier's sphere is resolved against whole polygons, the
// nearest one first. The terrain is a height field: a sphere touches it when
// its bottom dips below the ground straight under its center.
//
// Bodies are collision that comes and goes (a turret built on its pad, a
// droid's wreck): each is tested with the buildings while enabled, and a ray
// that hits one says which.
//
// rays: a RayKernel (src/ray-kernel.js) to answer cast, castMoving, the
// ground's ray, gather, overlapsSphere and terrainNear in WebAssembly, with
// the same answers. A kernel is required for every world.
export class CollisionWorld {
  constructor(positions, indices, polygons, terrainPositions, terrainIndices, rays) {
    if (!rays) throw Error("Collision world requires the Rust ray kernel");
    this.bvh = buildBVH(positions, indices, rays);
    this.polygonOf = polygons;
    this.positions = positions;
    this.indices = indices;
    this.triBounds = triangleBounds(positions, indices);
    this.staticTriangles = new Array(indices.length / 3);
    this.terrain = terrainIndices?.length ? buildBVH(terrainPositions, terrainIndices, rays) : null;
    this.terrainGrid = terrainIndices?.length ? new TriangleGrid(terrainPositions, terrainIndices) : null;
    this.lastGround = { x: NaN, z: NaN, top: NaN, hit: false, height: 0, normal: new THREE.Vector3() };
    this.box = new THREE.Box3();
    this.triBox = new THREE.Box3();
    this.ray = new THREE.Ray();
    this.down = new THREE.Vector3(0, -1, 0);
    this.localRay = new THREE.Ray();
    this.worldBox = new THREE.Box3();
    this.localCenter = new THREE.Vector3();
    this.nearest = new THREE.Vector3();
    this.pool = [];
    this.used = 0;
    this.polygonPool = [];
    this.polygonUsed = 0;
    // The gather's polygons by number once its list is long (polygon()).
    sceneProperty(this,"groups",new PolygonIndex());
    this.grouped = false;
    this.list = [];
    this.bodies = [];
    this.bodySerial = 0;
    // Bumped whenever a body is added or removed (gather caches).
    this.bodiesVersion = 0;
    this.vehicleBlockers = null;
    // The buildings, and the vehicle-only barriers once a cached gather
    // asks for them (blockerSet): fixed triangles a cache files by number.
    this.staticSet = { shape: null, polygonOf: polygons, positions, indices, triBounds: this.triBounds, triangles: this.staticTriangles, offset: 0 };
    this.blockers = null;
    // The scratch state of the query running now.
    this.gatherMatrix = null;
    this.gatherPolygonOf = polygons;
    this.gatherOffset = 0;
    this.filling = null;
    this.fillBounds = null;
    this.overlapCenter = null;
    this.overlapRadiusSq = 0;
    this.terrainNearList = [];
    this.terrainNearPool = [];
    this.rays = rays;
    {
      this.rayGrid = this.terrainGrid ? rays.grid(this.terrainGrid) : -1;
      this.rayWorld = rays.world(rays.tree(this.bvh), this.terrain ? rays.tree(this.terrain) : -1, this.rayGrid);
      rays.x.world_statics(this.rayWorld, rays.fixedSet(this.staticSet, this.bvh));
      // The bodies' order the module last had (bodiesVersion).
      this.rayOrder = -1;
      this.rayGround = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3() };
    }
    this.movement = new MovementKernel(this);
  }

    // A body's copy in the ray kernel: its tree, and its place (a moving
  // body's as it moves, in moveBody).
  rayBody(body, moving) {
    body.raySlot = this.rays.body(body, moving);
    if (!moving) this.rays.place(body.raySlot, body.bounds, null, null);
  }

  // The ray kernel's list of bodies, when bodies came or went since.
  rayBodies() {
    if (this.rayOrder === this.bodiesVersion) return;
    this.rays.order(this.rayWorld, this.bodies);
    this.rayOrder = this.bodiesVersion;
  }

  // The bodies the cast whose ray is in the kernel must try (its own tests:
  // the ignored body, static-only, moving-only, bounds), and whether each is
  // enabled now. Props, traps and hulls switch bodies on and off by setting
  // `enabled`, a plain field the kernel cannot see, so it is read here for
  // these few. (An accessor that told the kernel would give each body its
  // own hidden class, and slow every loop over bodies down.)
  rayCandidates(ignoreBody, staticOnly, movingOnly) {
    const rays = this.rays, bodies = this.bodies;
    const count = rays.x.world_candidates(this.rayWorld, ignoreBody?.raySlot ?? -1, staticOnly ? 1 : 0, movingOnly ? 1 : 0);
    if (count) {
      const candidates = rays.candidates(), enabled = rays.flags();
      for (let c = 0; c < count; c++) enabled[c] = bodies[candidates[c]].enabled ? 1 : 0;
    }
    return count;
  }

  // A body from triangles (indices into positions) and each triangle's
  // polygon number. Starts enabled.
  addBody(positions, indices, polygons, owner = null) {
    const body = {
      bvh: buildBVH(positions, indices, this.rays),
      polygonOf: polygons,
      // Polygon numbers of their own, apart from the buildings'.
      offset: (++this.bodySerial) * 0x1000000,
      enabled: true,
      owner,
    };
    // Live props toggle between static world-space meshes; their triangle
    // bounds remain valid while enabled/disabled states change.
    body.bounds = body.bvh.getBoundingBox(new THREE.Box3());
    this.bodies.push(body);
    this.bodiesVersion++;
    this.rayBody(body, false);
    return body;
  }

  // Rigid moving bodies share an immutable local-space BVH. Movement updates
  // only the two transforms and the broad-phase bounds, never the triangles.
  createBodyShape(positions, indices, polygons) {
    const bvh = buildBVH(positions, indices, this.rays);
    return { bvh, polygonOf: polygons, localBounds: bvh.getBoundingBox(new THREE.Box3()) };
  }

  addMovingBody(shape, matrix, owner = null) {
    const body = { ...shape, matrix: new THREE.Matrix4(), inverse: new THREE.Matrix4(),
      bounds: new THREE.Box3(), offset: (++this.bodySerial) * 0x1000000,
      enabled: true, moving: true, owner };
    this.bodies.push(body);
    this.bodiesVersion++;
    this.rayBody(body, true);
    this.moveBody(body, matrix);
    return body;
  }

  moveBody(body, matrix) {
    sceneTransform(body.matrix.copy(matrix));
    body.inverse.copy(matrix).invert();
    sceneTransform(body.bounds.copy(body.localBounds).applyMatrix4(matrix));
    this.rays.place(body.raySlot, body.bounds, body.matrix, body.inverse);
  }

  removeBody(body) {
    const index = this.bodies.indexOf(body);
    if (index >= 0) this.bodies.splice(index, 1);
    this.bodiesVersion++;
    body.enabled = false;
  }

  // Test only moving objects over a projectile's current flight segment.
  // Static impacts remain precomputed; a moving target is never snapshotted.
  castMoving(origin, direction, far, out, ignoreBody = null) {
    return this.rayCastMoving(origin, direction, far, out, ignoreBody);
  }

  // Polygons with a triangle whose bounds overlap the sphere's bounds, each as a
  // list of triangles. The result is reused by the next call.
  //
  // A caller that queries around the same spot every tick (a soldier's body
  // sphere, a hull's wall probe) can pass a cache from gatherCache(). The
  // buildings and the vehicle-only barriers never change, so their triangles
  // near the sphere are fetched from their trees once, with a margin, and
  // each later query only filters those short lists with the same bounds
  // test, until the sphere leaves the margin. A tree visits triangles in a
  // fixed order and the filter keeps it, so the polygons and their triangles
  // come out exactly as without the cache.
  // vehicle: the vehicle-only barriers count too (a hull's query).
  // ignoreBody: a body whose polygons are left out (a dead rider's mount).
  gather(center, radius, vehicle = false, cache = null, ignoreBody = null) {
    return this.rayGather(center, radius, vehicle, cache, ignoreBody);
  }

  // The current gather's record of polygon `id` (a new one, at the end of
  // the list, the first time). A gather finds few polygons, and a
  // polygon's triangles come together, so the list is searched from its
  // end; past GROUP_SCAN polygons an index of their places in the list takes
  // over, emptied only after a gather that used it.
  polygon(id, moving) {
    const list = this.list;
    if (this.grouped) {
      const at = this.groups.get(id);
      if (at >= 0) return list[at];
    } else {
      for (let i = list.length - 1; i >= 0; i--) if (list[i].id === id) return list[i];
      if (list.length >= GROUP_SCAN) {
        for (let i = 0; i < list.length; i++) this.groups.set(list[i].id, i);
        this.grouped = true;
      }
    }
    if (this.polygonUsed === this.polygonPool.length) this.polygonPool.push({ id: 0, triangles: [], done: false, moving: false });
    const polygon = this.polygonPool[this.polygonUsed++];
    polygon.id = id;
    empty(polygon.triangles);
    polygon.done = false;
    polygon.moving = moving;
    list.push(polygon);
    if (this.grouped) this.groups.set(id, list.length - 1);
    return polygon;
  }

  gatherCache(margin = 0.75) {
    return { box: new THREE.Box3().makeEmpty(), margin, fill: 0,
      // The buildings' triangles in the box, and the vehicle-only barriers'
      // for a hull's query (set: the barriers they were filled from).
      statics: { set: null, list: new Uint32Array(64), count: 0 },
      blockers: { set: null, list: new Uint32Array(16), count: 0 },
      bodies: [], bodiesVersion: -1, bodiesFill: -1,
      // Its copy in the ray kernel, once a query through it runs there.
      ray: -1 };
  }

  // blockers: the vehicle-only barriers too (a hull's query), else null.

  // A cache's triangles of a fixed set whose bounds meet the query's box,
  // gather, in the ray kernel: the same polygons in the same order, each
  // with the same triangles (a fixed set's own Triangles, as gatherSet
  // gives them; a body's copied into the pool, as triangle() does). Bodies'
  // `enabled` is read here for the few the sphere can meet (rayCandidates).
  rayGather(center, radius, vehicle, cache, ignoreBody) {
    this.rayBodies();
    const rays = this.rays, io = rays.view(), world = this.rayWorld;
    io[0] = center.x; io[1] = center.y; io[2] = center.z; io[3] = radius;
    const blockers = vehicle && this.vehicleBlockers ? this.blockerSet(this.vehicleBlockers) : null;
    const count = rays.x.gather_begin(world, cache ? rays.cache(cache) : -1, blockers ? rays.fixedSet(blockers, blockers.shape.bvh) : -1, ignoreBody?.raySlot ?? -1);
    this.rayEnabled(count);
    const polygons = rays.x.gather_end(world, count);
    // Both calls may have grown the module's memory (views made before
    // are empty then): the output's address is read from a fresh view.
    const out = rays.outF64(rays.view()[58]), list = this.list, pool = this.pool, polygonPool = this.polygonPool, sets = rays.sets;
    this.used = 0;
    this.polygonUsed = 0;
    empty(list);
    for (let p = 0, k = 0; p < polygons; p++) {
      if (this.polygonUsed === polygonPool.length) polygonPool.push({ id: 0, triangles: [], done: false, moving: false });
      const polygon = polygonPool[this.polygonUsed++], triangles = polygon.triangles;
      polygon.id = out[k];
      empty(triangles);
      polygon.done = false;
      polygon.moving = out[k + 1] !== 0;
      list.push(polygon);
      const n = out[k + 2];
      k += 3;
      for (let t = 0; t < n; t++) {
        const set = out[k], index = out[k + 1];
        if (set >= 0) {
          const fixed = sets[set];
          triangles.push(fixed.triangles[index] || fixedTriangle(fixed, index));
          k += 2;
        } else {
          if (this.used === pool.length) pool.push(new THREE.Triangle());
          const tri = pool[this.used++];
          tri.a.set(out[k + 2], out[k + 3], out[k + 4]);
          tri.b.set(out[k + 5], out[k + 6], out[k + 7]);
          tri.c.set(out[k + 8], out[k + 9], out[k + 10]);
          triangles.push(tri);
          k += 11;
        }
      }
    }
    return list;
  }

  // A hull's wall probes (vehicles.js pushWalls) in the ray kernel, whole:
  // each probe's gather (through its cache) and terrain triangles, and every
  // push out of them, in pushWalls' order; position and velocity are set
  // to where they leave the hull, and true comes back if its turn is to be
  // undone (a push deeper than 0.15 m). q, own: Quaternions (own null
  // unless the hull walks); previous: its place a step ago.
  rayPushWalls(probes, radius, caches, q, own, previous, position, velocity, terrain) {
    const rays = this.rays, bodies = this.bodies, n = probes.length;
    this.rayBodies();
    // Ids first: making a cache or a set may grow the module's memory.
    const blockers = this.vehicleBlockers ? rays.fixedSet(this.blockerSet(this.vehicleBlockers), this.vehicleBlockers.bvh) : -1;
    for (let i = 0; i < n; i++) if (caches[i]) rays.cache(caches[i]);
    const input = rays.input(22 + n * 4);
    input[0] = q.x; input[1] = q.y; input[2] = q.z; input[3] = q.w;
    if (own) { input[4] = own.x; input[5] = own.y; input[6] = own.z; input[7] = own.w; input[8] = 1; } else input[8] = 0;
    input[9] = previous.x; input[10] = previous.y; input[11] = previous.z;
    input[12] = position.x; input[13] = position.y; input[14] = position.z;
    input[15] = velocity.x; input[16] = velocity.y; input[17] = velocity.z;
    input[18] = radius; input[19] = terrain ? 1 : 0; input[20] = blockers; input[21] = n;
    for (let i = 0, o = 22; i < n; i++, o += 4) {
      const p = probes[i];
      input[o] = p.x; input[o + 1] = p.y; input[o + 2] = p.z; input[o + 3] = caches[i] ? caches[i].ray : -1;
    }
    const places = rays.places();
    for (let i = 0, n = Math.min(bodies.length, places.length); i < n; i++) places[i] = bodies[i].enabled ? 1 : 0;
    rays.x.push_walls_ray(this.rayWorld);
    const io = rays.view();
    position.set(io[0], io[1], io[2]);
    velocity.set(io[3], io[4], io[5]);
    return io[6] !== 0;
  }

  // Whether each of the `count` bodies the kernel's query just named is
  // enabled (ENABLED), read from the bodies' plain field.
  rayEnabled(count) {
    if (!count) return;
    const rays = this.rays, bodies = this.bodies, candidates = rays.candidates(), enabled = rays.flags();
    for (let c = 0; c < count; c++) enabled[c] = bodies[candidates[c]].enabled ? 1 : 0;
  }

  // The vehicle-only barriers (Vehicles sets vehicleBlockers, a body shape
  // in world space) as a fixed set for cached gathers.
  blockerSet(shape) {
    if (this.blockers?.shape !== shape) {
      const geometry = shape.bvh.geometry, positions = geometry.attributes.position.array, indices = geometry.index.array;
      this.blockers = { shape, polygonOf: shape.polygonOf, positions, indices, triBounds: triangleBounds(positions, indices),
        triangles: new Array(indices.length / 3), offset: BLOCKER_POLYGONS };
    }
    return this.blockers;
  }

  triangle(tri) {
    if (this.used === this.pool.length) this.pool.push(new THREE.Triangle());
    const out = this.pool[this.used++];
    out.a.copy(tri.a);
    out.b.copy(tri.b);
    out.c.copy(tri.c);
    if (this.gatherMatrix) {
      sceneTransform(out.a.applyMatrix4(this.gatherMatrix));
      sceneTransform(out.b.applyMatrix4(this.gatherMatrix));
      sceneTransform(out.c.applyMatrix4(this.gatherMatrix));
    }
    return out;
  }

  // Clearance for a swept posture, including live props. Unlike gather this
  // does not disturb the polygon contact list currently being resolved.
  overlapsSphere(center, radius, ignoreBody = null) {
    return this.rayOverlapsSphere(center, radius, ignoreBody);
  }

  // overlapsSphere, in the ray kernel: the buildings and the terrain in the
  // first call, the enabled bodies it names in the second.
  rayOverlapsSphere(center, radius, ignoreBody) {
    this.rayBodies();
    const rays = this.rays, io = rays.view();
    io[0] = center.x; io[1] = center.y; io[2] = center.z; io[3] = radius;
    const count = rays.x.overlap_begin(this.rayWorld, ignoreBody?.raySlot ?? -1);
    if (count < 0) return true;
    if (count === 0) return false;
    this.rayEnabled(count);
    return rays.x.overlap_end(this.rayWorld, count) !== 0;
  }

  // Ground height and face normal straight under (x, z), searched from `top`
  // down. Null where there is no terrain (off the map or in a cut).
  ground(x, z, top, out) {
    if (!this.terrainGrid) return null;
    // Standing/crouching body spheres often ask for the exact same ground
    // column. Terrain is immutable; cache only that one exact query, including
    // its ceiling. Any lateral collision adjustment forces a fresh query.
    const cached = this.lastGround;
    if (cached.x === x && cached.z === z && cached.top === top) {
      if (!cached.hit) return null;
      out.height = cached.height;
      out.normal.copy(cached.normal);
      return out;
    }
    cached.x = x; cached.z = z; cached.top = top;
    this.ray.origin.set(x, top, z);
    this.ray.direction.copy(this.down);
    const hit = this.rayGroundHit();
    cached.hit = Boolean(hit);
    if (!hit) return null;
    out.height = hit.point.y;
    out.normal.copy(hit.normal);
    if (out.normal.y < 0) out.normal.negate();
    cached.height = out.height;
    cached.normal.copy(out.normal);
    return out;
  }

  // The terrain's triangles near a sphere (those filed in the ground cells
  // its bounds cover), as Triangles the next call reuses: a hull meets the
  // terrain's steep faces as walls (Vehicle.pushWalls).
  terrainNear(center, radius) {
    const out = this.terrainNearList;
    empty(out);
    if (this.terrainGrid) this.rayTerrainNear(center, radius, out, this.terrainNearPool);
    return out;
  }

  // TriangleGrid.near, in the ray kernel: the triangles' numbers from the
  // module, their corners copied as near() copies them.
  rayTerrainNear(center, radius, out, pool) {
    const rays = this.rays, io = rays.view(), grid = this.terrainGrid, p = grid.positions, ix = grid.indices;
    io[0] = center.x; io[1] = center.y; io[2] = center.z; io[3] = radius;
    const count = rays.x.terrain_near_ray(this.rayGrid);
    const near = rays.outU32(rays.view()[58]);
    for (let i = 0; i < count; i++) {
      if (out.length === pool.length) pool.push(new THREE.Triangle());
      const tri = pool[out.length], t = near[i] * 3;
      tri.a.fromArray(p, ix[t] * 3);
      tri.b.fromArray(p, ix[t + 1] * 3);
      tri.c.fromArray(p, ix[t + 2] * 3);
      out.push(tri);
    }
  }

  // First hit along a ray, against buildings, bodies, and terrain. The normal
  // faces back toward the ray's origin; `terrain` says the terrain was hit,
  // and `body` which body, if one was. A firing turret can exclude its own
  // live body without excluding nearby architecture or other turrets.
  cast(origin, direction, far, out, ignoreBody = null, staticOnly = false) {
    return this.rayCast(origin, direction, far, out, ignoreBody, staticOnly);
  }

  // cast, in the ray kernel: `out` and the world's ray as cast leaves them.
  // `far` must be a number, as every caller's is (the JavaScript would take
  // an undefined one as Infinity; the kernel's slot would hold NaN).
  rayCast(origin, direction, far, out, ignoreBody, staticOnly) {
    this.ray.origin.copy(origin);
    this.ray.direction.copy(direction);
    this.rayBodies();
    const rays = this.rays, io = rays.view();
    io[0] = origin.x; io[1] = origin.y; io[2] = origin.z;
    io[3] = direction.x; io[4] = direction.y; io[5] = direction.z;
    // castTerrain's choice of grid or tree reads Math.hypot, V8's own.
    io[6] = far; io[7] = Math.hypot(direction.x, direction.z);
    const code = rays.x.world_cast(this.rayWorld, this.rayCandidates(ignoreBody, staticOnly, false));
    out.body = null;
    if (code === RAY_MISS) { out.terrain = false; return null; }
    out.terrain = code === RAY_TERRAIN;
    if (code >= 0) out.body = rays.bodies[code];
    // The hit through a fresh view: naming the candidates may have grown the
    // module's memory (its broad phase made again), emptying views made before.
    const hit = rays.view();
    out.distance = hit[10];
    out.point.set(hit[11], hit[12], hit[13]);
    out.normal.set(hit[14], hit[15], hit[16]);
    return out;
  }

  // A block of `count` CastRay records at once (rust/sim collision
  // world_cast_rays): each ray's first hit, as cast() finds it with the
  // record's body left out, its hit and distance written back. The bodies'
  // enabled flags go to the kernel once for the block.
  castRays(records, count) {
    this.rayBodies();
    const rays = this.rays, words = count * CR.STRIDE, bodies = this.bodies;
    const places = rays.places();
    for (let i = 0, n = Math.min(bodies.length, places.length); i < n; i++) places[i] = bodies[i].enabled ? 1 : 0;
    let input = rays.input(words);
    for (let i = 0; i < words; i++) input[i] = records[i];
    rays.x.world_cast_rays(this.rayWorld, count);
    input = rays.input(words);
    for (let r = 0, at = 0; r < count; r++, at += CR.STRIDE) { records[at + CR.HIT] = input[at + CR.HIT]; records[at + CR.DISTANCE] = input[at + CR.DISTANCE]; }
  }

  // castMoving, in the ray kernel.
  rayCastMoving(origin, direction, far, out, ignoreBody) {
    this.ray.set(origin, direction);
    this.rayBodies();
    const rays = this.rays, io = rays.view();
    io[0] = origin.x; io[1] = origin.y; io[2] = origin.z;
    io[3] = direction.x; io[4] = direction.y; io[5] = direction.z;
    io[6] = far;
    const code = rays.x.world_cast_moving(this.rayWorld, this.rayCandidates(ignoreBody, false, true));
    if (code === RAY_MISS) return null;
    const hit = rays.view();
    out.point.set(hit[11], hit[12], hit[13]);
    out.normal.set(hit[14], hit[15], hit[16]);
    out.distance = hit[10]; out.body = rays.bodies[code]; out.terrain = false;
    return out;
  }

  // The terrain grid's first hit along the world's ray (ground), in the ray
  // kernel, as { distance, point, normal } the next call reuses.
  rayGroundHit() {
    const io = this.rays.view(), o = this.ray.origin, d = this.ray.direction;
    io[0] = o.x; io[1] = o.y; io[2] = o.z; io[3] = d.x; io[4] = d.y; io[5] = d.z; io[6] = Infinity;
    if (!this.rays.x.grid_ray(this.rayGrid)) return null;
    const hit = this.rayGround, at = this.rays.view();
    hit.distance = at[10];
    hit.point.set(at[11], at[12], at[13]);
    hit.normal.set(at[14], at[15], at[16]);
    return hit;
  }
}

// A fixed set's Triangle for triangle `index`, made the first time it is
// gathered (gatherSet) and kept.
function fixedTriangle(set, index) {
  const out = set.triangles[index] = new THREE.Triangle(), p = set.positions, ix = set.indices, t = index * 3;
  out.a.fromArray(p, ix[t] * 3);
  out.b.fromArray(p, ix[t + 1] * 3);
  out.c.fromArray(p, ix[t + 2] * 3);
  return out;
}

// Each triangle's bounds: min x, y, z, then max x, y, z.
function triangleBounds(positions, indices) {
  const out = new Float32Array(indices.length * 2);
  for (let t = 0, o = 0; t < indices.length; t += 3, o += 6) {
    const a = indices[t] * 3, b = indices[t + 1] * 3, c = indices[t + 2] * 3;
    for (let k = 0; k < 3; k++) {
      out[o + k] = Math.min(positions[a + k], positions[b + k], positions[c + k]);
      out[o + 3 + k] = Math.max(positions[a + k], positions[b + k], positions[c + k]);
    }
  }
  return out;
}

// The terrain's triangles filed by the ground cells (x, z) their bounds
// cover. "Ground under me" tests the few triangles of one cell, and a ray
// walks only the cells it crosses, nearest first, stopping once a hit lies
// within the cell already walked. Each triangle gets the tree's own test
// (three's ray-triangle intersection, either side), so the answers match it.
class TriangleGrid {
  constructor(positions, indices) {
    this.positions = positions;
    this.indices = indices;
    const count = indices.length / 3;
    const bounds = triangleBounds(positions, indices);
    let minX = Infinity, minZ = Infinity, maxX = -Infinity, maxZ = -Infinity, area = 0;
    for (let o = 0; o < bounds.length; o += 6) {
      minX = Math.min(minX, bounds[o]); minZ = Math.min(minZ, bounds[o + 2]);
      maxX = Math.max(maxX, bounds[o + 3]); maxZ = Math.max(maxZ, bounds[o + 5]);
      area += (bounds[o + 3] - bounds[o]) * (bounds[o + 5] - bounds[o + 2]);
    }
    // Cells about the size of a typical triangle, at most ~1M of them.
    let size = Math.max(0.5, Math.sqrt(area / Math.max(1, count)));
    size = Math.max(size, Math.sqrt((maxX - minX) * (maxZ - minZ) / (1 << 20)));
    // Bounds grow by a hair so a point on a cell edge still finds the
    // triangles that rounding might let a ray touch.
    const pad = 1e-3;
    this.minX = minX - pad; this.minZ = minZ - pad; this.size = size;
    this.nx = Math.max(1, Math.ceil((maxX + pad - this.minX) / size));
    this.nz = Math.max(1, Math.ceil((maxZ + pad - this.minZ) / size));
    this.maxX = this.minX + this.nx * size; this.maxZ = this.minZ + this.nz * size;
    const cell = (v, min, n) => Math.min(n - 1, Math.max(0, Math.floor((v - min) / size)));
    const start = new Uint32Array(this.nx * this.nz + 1);
    const each = (fn) => {
      for (let t = 0, o = 0; t < count; t++, o += 6) {
        const x0 = cell(bounds[o] - pad, this.minX, this.nx), x1 = cell(bounds[o + 3] + pad, this.minX, this.nx);
        const z0 = cell(bounds[o + 2] - pad, this.minZ, this.nz), z1 = cell(bounds[o + 5] + pad, this.minZ, this.nz);
        for (let z = z0; z <= z1; z++) for (let x = x0; x <= x1; x++) fn(z * this.nx + x, t);
      }
    };
    each((c) => { start[c + 1]++; });
    for (let c = 0; c < this.nx * this.nz; c++) start[c + 1] += start[c];
    const fill = start.slice(0, -1);
    this.cells = new Uint32Array(start[this.nx * this.nz]);
    each((c, t) => { this.cells[fill[c]++] = t; });
    this.start = start;
    this.stamp = new Uint32Array(count);
    this.serial = 0;
    this.a = new THREE.Vector3(); this.b = new THREE.Vector3(); this.c = new THREE.Vector3();
    this.triangle = new THREE.Triangle(this.a, this.b, this.c);
    this.point = new THREE.Vector3();
    this.closest = new THREE.Vector3();
    this.hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3() };
  }

}

function buildBVH(positions, indices, rays) {
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(positions, 3));
  geometry.setIndex(new THREE.BufferAttribute(indices, 1));
  // Indirect keeps the triangle order, so triangle numbers still match the
  // polygon list.
  return rays.buildBVH(geometry);
}

export function makeHit() {
  return { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: null };
}

export function makeGround() {
  return { height: 0, normal: new THREE.Vector3() };
}
