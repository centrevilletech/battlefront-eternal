import * as THREE from "three";
import { MeshBVH } from "three-mesh-bvh";

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
export class CollisionWorld {
  constructor(positions, indices, polygons, terrainPositions, terrainIndices) {
    this.bvh = buildBVH(positions, indices);
    this.polygonOf = polygons;
    this.positions = positions;
    this.indices = indices;
    this.triBounds = triangleBounds(positions, indices);
    this.staticTriangles = new Array(indices.length / 3);
    this.terrain = terrainIndices?.length ? buildBVH(terrainPositions, terrainIndices) : null;
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
    this.groups = new Map();
    this.list = [];
    this.bodies = [];
    this.bodySerial = 0;
    // Bumped whenever a body is added or removed (gather caches).
    this.bodiesVersion = 0;
    this.vehicleBlockers = null;
    // Queries run synchronously. Keep their callbacks and contact records
    // across ticks instead of allocating them for every body and sphere.
    this.gatherQuery = {
      intersectsBounds: (box) => box.intersectsBox(this.box),
      intersectsTriangle: (tri, index) => {
        this.triBox.min.copy(tri.a).min(tri.b).min(tri.c);
        this.triBox.max.copy(tri.a).max(tri.b).max(tri.c);
        if (!this.triBox.intersectsBox(this.box)) return false;
        const id = this.gatherPolygonOf[index] + this.gatherOffset;
        let polygon = this.groups.get(id);
        if (!polygon) {
          if (this.polygonUsed === this.polygonPool.length) this.polygonPool.push({ triangles: [], done: false, moving: false });
          polygon = this.polygonPool[this.polygonUsed++];
          polygon.triangles.length = 0;
          polygon.done = false;
          polygon.moving = this.gatherMatrix !== null;
          this.groups.set(id, polygon);
          this.list.push(polygon);
        }
        polygon.triangles.push(this.triangle(tri));
        return false;
      },
    };
    // Fills a caller's cache with every building triangle near its query,
    // in the tree's own order (see gather).
    this.cacheQuery = {
      intersectsBounds: (box) => box.intersectsBox(this.cacheBox),
      intersectsTriangle: (tri, index) => {
        const b = this.triBounds, o = index * 6, box = this.cacheBox;
        if (b[o + 3] < box.min.x || b[o] > box.max.x || b[o + 4] < box.min.y || b[o + 1] > box.max.y
          || b[o + 5] < box.min.z || b[o + 2] > box.max.z) return false;
        const cache = this.filling;
        if (cache.count === cache.list.length) {
          const grown = new Uint32Array(cache.list.length * 2);
          grown.set(cache.list);
          cache.list = grown;
        }
        cache.list[cache.count++] = index;
        return false;
      },
    };
    this.cacheBox = new THREE.Box3();
    this.overlapQuery = {
      intersectsBounds: (box) => box.intersectsBox(this.box),
      intersectsTriangle: (tri) => {
        tri.closestPointToPoint(this.overlapCenter, this.nearest);
        return this.nearest.distanceToSquared(this.overlapCenter) < this.overlapRadiusSq;
      },
    };
  }

  // A body from triangles (indices into positions) and each triangle's
  // polygon number. Starts enabled.
  addBody(positions, indices, polygons, owner = null) {
    const body = {
      bvh: buildBVH(positions, indices),
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
    return body;
  }

  // Rigid moving bodies share an immutable local-space BVH. Movement updates
  // only the two transforms and the broad-phase bounds, never the triangles.
  createBodyShape(positions, indices, polygons) {
    const bvh = buildBVH(positions, indices);
    return { bvh, polygonOf: polygons, localBounds: bvh.getBoundingBox(new THREE.Box3()) };
  }

  addMovingBody(shape, matrix, owner = null) {
    const body = { ...shape, matrix: new THREE.Matrix4(), inverse: new THREE.Matrix4(),
      bounds: new THREE.Box3(), offset: (++this.bodySerial) * 0x1000000,
      enabled: true, moving: true, owner };
    this.bodies.push(body);
    this.bodiesVersion++;
    this.moveBody(body, matrix);
    return body;
  }

  moveBody(body, matrix) {
    body.matrix.copy(matrix);
    body.inverse.copy(matrix).invert();
    body.bounds.copy(body.localBounds).applyMatrix4(matrix);
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
    this.ray.set(origin, direction);
    let closest = far;
    let found = false;
    for (const body of this.bodies) {
      if (!body.moving || !body.enabled || body === ignoreBody || !this.ray.intersectsBox(body.bounds)) continue;
      this.localRay.copy(this.ray).applyMatrix4(body.inverse);
      const hit = body.bvh.raycastFirst(this.localRay, THREE.DoubleSide, 0, closest);
      if (!hit) continue;
      closest = hit.distance;
      out.point.copy(hit.point).applyMatrix4(body.matrix);
      out.normal.copy(hit.face.normal).transformDirection(body.matrix);
      if (out.normal.dot(direction) > 0) out.normal.negate();
      out.distance = closest; out.body = body; out.terrain = false;
      found = true;
    }
    return found ? out : null;
  }

  // Polygons with a triangle whose bounds overlap the sphere's bounds, each as a
  // list of triangles. The result is reused by the next call.
  //
  // A caller that queries around the same spot every tick (a soldier's body
  // sphere) can pass a cache from gatherCache(). The buildings never change,
  // so their triangles near the sphere are fetched from the tree once, with a
  // margin, and each later query only filters that short list with the same
  // bounds test, until the sphere leaves the margin. The tree visits
  // triangles in a fixed order and the filter keeps it, so the polygons and
  // their triangles come out exactly as without the cache.
  // ignoreBody: a body whose polygons are left out (a dead rider's mount).
  gather(center, radius, vehicle = false, cache = null, ignoreBody = null) {
    this.box.min.set(center.x - radius, center.y - radius, center.z - radius);
    this.box.max.set(center.x + radius, center.y + radius, center.z + radius);
    this.used = 0;
    this.polygonUsed = 0;
    this.groups.clear();
    this.list.length = 0;
    this.gatherMatrix = null;
    if (cache) this.gatherCached(cache);
    else this.gatherFrom(this.bvh, this.polygonOf, 0);
    if (vehicle && this.vehicleBlockers) this.gatherFrom(this.vehicleBlockers.bvh, this.vehicleBlockers.polygonOf, 0x70000000);
    this.worldBox.copy(this.box);
    for (const body of cache ? this.cachedBodies(cache) : this.bodies) {
      if (!body.enabled || body === ignoreBody || !body.bounds.intersectsBox(this.worldBox)) continue;
      this.gatherMatrix = body.matrix || null;
      if (body.inverse) this.box.copy(this.worldBox).applyMatrix4(body.inverse);
      this.gatherFrom(body.bvh, body.polygonOf, body.offset);
      this.box.copy(this.worldBox);
    }
    this.gatherMatrix = null;
    return this.list;
  }

  gatherCache(margin = 0.75) {
    return { box: new THREE.Box3().makeEmpty(), list: new Uint32Array(64), count: 0, margin,
      bodies: [], bodiesVersion: -1, bodiesFill: -1, fill: 0 };
  }

  // The bodies a cached query can meet, in the world's order: every moving
  // one, and the fixed ones whose bounds meet the cache's box.
  cachedBodies(cache) {
    if (cache.bodiesVersion !== this.bodiesVersion || cache.bodiesFill !== cache.fill) {
      cache.bodies.length = 0;
      for (const body of this.bodies) if (body.moving || body.bounds.intersectsBox(cache.box)) cache.bodies.push(body);
      cache.bodiesVersion = this.bodiesVersion;
      cache.bodiesFill = cache.fill;
    }
    return cache.bodies;
  }

  gatherCached(cache) {
    const box = this.box;
    if (!cache.box.containsBox(box)) {
      this.cacheBox.copy(box).expandByScalar(cache.margin);
      cache.box.copy(this.cacheBox);
      cache.count = 0;
      cache.fill++;
      this.filling = cache;
      this.bvh.shapecast(this.cacheQuery);
      this.filling = null;
    }
    const b = this.triBounds, p = this.positions, ix = this.indices, list = cache.list;
    const minX = box.min.x, minY = box.min.y, minZ = box.min.z, maxX = box.max.x, maxY = box.max.y, maxZ = box.max.z;
    for (let i = 0; i < cache.count; i++) {
      const index = list[i], o = index * 6;
      if (b[o + 3] < minX || b[o] > maxX || b[o + 4] < minY || b[o + 1] > maxY || b[o + 5] < minZ || b[o + 2] > maxZ) continue;
      const id = this.polygonOf[index];
      let polygon = this.groups.get(id);
      if (!polygon) {
        if (this.polygonUsed === this.polygonPool.length) this.polygonPool.push({ triangles: [], done: false, moving: false });
        polygon = this.polygonPool[this.polygonUsed++];
        polygon.triangles.length = 0;
        polygon.done = false;
        polygon.moving = false;
        this.groups.set(id, polygon);
        this.list.push(polygon);
      }
      // Cached building triangles never move. Materialize each encountered
      // triangle once instead of copying nine coordinates per body sphere
      // every physics tick. Moving hulls still use the transient world pool.
      let out = this.staticTriangles[index];
      if (!out) {
        out = this.staticTriangles[index] = new THREE.Triangle();
        const t = index * 3;
        out.a.fromArray(p, ix[t] * 3);
        out.b.fromArray(p, ix[t + 1] * 3);
        out.c.fromArray(p, ix[t + 2] * 3);
      }
      polygon.triangles.push(out);
    }
  }

  gatherFrom(bvh, polygonOf, offset) {
    this.gatherPolygonOf = polygonOf;
    this.gatherOffset = offset;
    bvh.shapecast(this.gatherQuery);
  }

  triangle(tri) {
    if (this.used === this.pool.length) this.pool.push(new THREE.Triangle());
    const out = this.pool[this.used++];
    out.a.copy(tri.a);
    out.b.copy(tri.b);
    out.c.copy(tri.c);
    if (this.gatherMatrix) {
      out.a.applyMatrix4(this.gatherMatrix);
      out.b.applyMatrix4(this.gatherMatrix);
      out.c.applyMatrix4(this.gatherMatrix);
    }
    return out;
  }

  // Clearance for a swept posture, including live props. Unlike gather this
  // does not disturb the polygon contact list currently being resolved.
  overlapsSphere(center, radius, ignoreBody = null) {
    this.box.min.copy(center).addScalar(-radius);
    this.box.max.copy(center).addScalar(radius);
    this.overlapCenter = center;
    this.overlapRadiusSq = radius * radius;
    if (this.bvh.shapecast(this.overlapQuery) || this.terrainGrid?.overlapsSphere(center, radius)) return true;
    this.worldBox.copy(this.box);
    for (const body of this.bodies) {
      if (!body.enabled || body === ignoreBody || !body.bounds.intersectsBox(this.worldBox)) continue;
      if (body.inverse) {
        this.box.copy(this.worldBox).applyMatrix4(body.inverse);
        this.overlapCenter = this.localCenter.copy(center).applyMatrix4(body.inverse);
      }
      const hit = body.bvh.shapecast(this.overlapQuery);
      this.box.copy(this.worldBox);
      this.overlapCenter = center;
      if (hit) return true;
    }
    return false;
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
    const hit = this.terrainGrid.raycastFirst(this.ray, Infinity);
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
    const out = this.terrainNearList ||= [];
    out.length = 0;
    this.terrainGrid?.near(center, radius, out, this.terrainNearPool ||= []);
    return out;
  }

  // The terrain's nearest hit: the grid for rays that cross few of its cells
  // (straight down, short), the tree for long ones. Same answer either way.
  castTerrain(ray, far) {
    const grid = this.terrainGrid;
    if (!grid) return null;
    const d = ray.direction;
    if (far * Math.hypot(d.x, d.z) / d.length() <= grid.size * 12) return grid.raycastFirst(ray, far);
    return this.terrain.raycastFirst(ray, THREE.DoubleSide, 0, far);
  }

  // First hit along a ray, against buildings, bodies, and terrain. The normal
  // faces back toward the ray's origin; `terrain` says the terrain was hit,
  // and `body` which body, if one was. A firing turret can exclude its own
  // live body without excluding nearby architecture or other turrets.
  cast(origin, direction, far, out, ignoreBody = null, staticOnly = false) {
    this.ray.origin.copy(origin);
    this.ray.direction.copy(direction);
    let hit = this.bvh.raycastFirst(this.ray, THREE.DoubleSide, 0, far);
    out.body = null;
    for (const body of this.bodies) {
      if (!body.enabled || body === ignoreBody || (staticOnly && body.moving) || !this.ray.intersectsBox(body.bounds)) continue;
      const ray = body.inverse ? this.localRay.copy(this.ray).applyMatrix4(body.inverse) : this.ray;
      const near = body.bvh.raycastFirst(ray, THREE.DoubleSide, 0, hit ? hit.distance : far);
      if (near && body.matrix) {
        near.point.applyMatrix4(body.matrix);
        near.face.normal.transformDirection(body.matrix);
      }
      if (near && (!hit || near.distance < hit.distance)) {
        hit = near;
        out.body = body;
      }
    }
    const low = this.castTerrain(this.ray, hit ? hit.distance : far);
    out.terrain = Boolean(low && (!hit || low.distance < hit.distance));
    if (out.terrain) {
      out.body = null;
      out.distance = low.distance;
      out.point.copy(low.point);
      out.normal.copy(low.face ? low.face.normal : low.normal);
    } else {
      if (!hit) return null;
      out.distance = hit.distance;
      out.point.copy(hit.point);
      out.normal.copy(hit.face.normal);
    }
    if (out.normal.dot(direction) > 0) out.normal.negate();
    return out;
  }
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

  load(t) {
    const p = this.positions, ix = this.indices, i = t * 3;
    this.a.fromArray(p, ix[i] * 3);
    this.b.fromArray(p, ix[i + 1] * 3);
    this.c.fromArray(p, ix[i + 2] * 3);
  }

  // Tests one cell's triangles not yet tested by this ray; returns the
  // nearest distance found so far.
  testCell(ray, cell, far, best) {
    const cells = this.cells, stamp = this.stamp, serial = this.serial;
    for (let i = this.start[cell], end = this.start[cell + 1]; i < end; i++) {
      const t = cells[i];
      if (stamp[t] === serial) continue;
      stamp[t] = serial;
      this.load(t);
      if (!ray.intersectTriangle(this.a, this.b, this.c, false, this.point)) continue;
      const distance = ray.origin.distanceTo(this.point);
      if (distance > far || !(distance < best)) continue;
      best = distance;
      this.hit.distance = distance;
      this.hit.point.copy(this.point);
      this.triangle.getNormal(this.hit.normal);
    }
    return best;
  }

  nextSerial() {
    if (++this.serial === 0xffffffff) { this.stamp.fill(0); this.serial = 1; }
  }

  // The nearest hit within `far`, as { distance, point, normal } (the
  // triangle's face normal, unflipped), or null. Reused by the next call.
  raycastFirst(ray, far) {
    const o = ray.origin, d = ray.direction, size = this.size;
    // Clip the ray to the grid's ground rectangle.
    let t0 = 0, t1 = Infinity;
    if (Math.abs(d.x) < 1e-12) {
      if (o.x < this.minX || o.x >= this.maxX) return null;
    } else {
      const a = (this.minX - o.x) / d.x, b = (this.maxX - o.x) / d.x;
      t0 = Math.max(t0, Math.min(a, b)); t1 = Math.min(t1, Math.max(a, b));
    }
    if (Math.abs(d.z) < 1e-12) {
      if (o.z < this.minZ || o.z >= this.maxZ) return null;
    } else {
      const a = (this.minZ - o.z) / d.z, b = (this.maxZ - o.z) / d.z;
      t0 = Math.max(t0, Math.min(a, b)); t1 = Math.min(t1, Math.max(a, b));
    }
    const length = d.length();
    if (length === 0) return null;
    t1 = Math.min(t1, far / length);
    if (!(t0 <= t1)) return null;
    this.nextSerial();
    let x = Math.min(this.nx - 1, Math.max(0, Math.floor((o.x + d.x * t0 - this.minX) / size)));
    let z = Math.min(this.nz - 1, Math.max(0, Math.floor((o.z + d.z * t0 - this.minZ) / size)));
    const stepX = d.x > 0 ? 1 : -1, stepZ = d.z > 0 ? 1 : -1;
    let nextX = Math.abs(d.x) < 1e-12 ? Infinity : (this.minX + (x + (stepX > 0 ? 1 : 0)) * size - o.x) / d.x;
    let nextZ = Math.abs(d.z) < 1e-12 ? Infinity : (this.minZ + (z + (stepZ > 0 ? 1 : 0)) * size - o.z) / d.z;
    const deltaX = Math.abs(d.x) < 1e-12 ? Infinity : size / Math.abs(d.x);
    const deltaZ = Math.abs(d.z) < 1e-12 ? Infinity : size / Math.abs(d.z);
    let best = Infinity;
    for (;;) {
      best = this.testCell(ray, z * this.nx + x, far, best);
      const exit = Math.min(nextX, nextZ);
      // A hit inside the walked cells cannot be beaten by a later cell.
      if (best <= exit * length || exit >= t1) break;
      if (nextX < nextZ) { x += stepX; nextX += deltaX; if (x < 0 || x >= this.nx) break; }
      else { z += stepZ; nextZ += deltaZ; if (z < 0 || z >= this.nz) break; }
    }
    return best < Infinity ? this.hit : null;
  }

  // The triangles filed in the cells a sphere's bounds cover, each once,
  // copied into pooled Triangles pushed to out.
  near(center, radius, out, pool) {
    const size = this.size;
    const x0 = Math.floor((center.x - radius - this.minX) / size), x1 = Math.floor((center.x + radius - this.minX) / size);
    const z0 = Math.floor((center.z - radius - this.minZ) / size), z1 = Math.floor((center.z + radius - this.minZ) / size);
    this.nextSerial();
    for (let z = Math.max(0, z0); z <= Math.min(this.nz - 1, z1); z++) {
      for (let x = Math.max(0, x0); x <= Math.min(this.nx - 1, x1); x++) {
        const cell = z * this.nx + x;
        for (let i = this.start[cell], end = this.start[cell + 1]; i < end; i++) {
          const t = this.cells[i];
          if (this.stamp[t] === this.serial) continue;
          this.stamp[t] = this.serial;
          if (out.length === pool.length) pool.push(new THREE.Triangle());
          const tri = pool[out.length];
          this.load(t);
          tri.a.copy(this.a); tri.b.copy(this.b); tri.c.copy(this.c);
          out.push(tri);
        }
      }
    }
    return out;
  }

  // Whether any triangle comes within `radius` of `center`.
  overlapsSphere(center, radius) {
    const size = this.size, r2 = radius * radius;
    const x0 = Math.floor((center.x - radius - this.minX) / size), x1 = Math.floor((center.x + radius - this.minX) / size);
    const z0 = Math.floor((center.z - radius - this.minZ) / size), z1 = Math.floor((center.z + radius - this.minZ) / size);
    this.nextSerial();
    for (let z = Math.max(0, z0); z <= Math.min(this.nz - 1, z1); z++) {
      for (let x = Math.max(0, x0); x <= Math.min(this.nx - 1, x1); x++) {
        const cell = z * this.nx + x;
        for (let i = this.start[cell], end = this.start[cell + 1]; i < end; i++) {
          const t = this.cells[i];
          if (this.stamp[t] === this.serial) continue;
          this.stamp[t] = this.serial;
          this.load(t);
          this.triangle.closestPointToPoint(center, this.closest);
          if (this.closest.distanceToSquared(center) < r2) return true;
        }
      }
    }
    return false;
  }
}

function buildBVH(positions, indices) {
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(positions, 3));
  geometry.setIndex(new THREE.BufferAttribute(indices, 1));
  // Indirect keeps the triangle order, so triangle numbers still match the
  // polygon list.
  return new MeshBVH(geometry, { indirect: true });
}

export function makeHit() {
  return { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: null };
}

export function makeGround() {
  return { height: 0, normal: new THREE.Vector3() };
}
