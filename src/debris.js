import * as THREE from "three";

// A droid blowing apart, as far as the 2004 game was traced.
//
// Each CHUNKSECTION of the class is a piece (ChunkGeometryName, modelled in
// its bone's space) that starts where its bone (ChunkNodeName) is. It flies
// at (the body's velocity + ChunkSpeed along the way out from the body's
// origin, tipped up by a quarter of that distance) x f, where f is 0.1 for a
// soldier killed at exactly 0 health, rising to 1 for one killed 50 or more
// below, and spins at ChunkOmega rad/s about each axis. It trails
// ChunkTrailEffect, and each of its first ChunkTerrainCollisions hits kicks
// up ChunkTerrainEffect. A vehicle's piece also smokes (ChunkSmokeEffect;
// the port lets it rise from the piece itself, not ChunkSmokeNodeName).
//
// The port's own (the original game's piece physics wasn't traced): pieces fall
// at 1 g and bounce like grenades, rest after their last counted hit or
// once slow, and are gone 10 s after they rest. A ChunkPhysics STATIC piece
// (the Techno Union ship's hulk) stays where it starts for good.
//
// A hull's pieces (centre: true) are modelled in the hull's frame, metres
// from its origin, which is on the ground under a walker. Each is carried
// by its own middle instead: it flies out from the hull along the way to
// that middle, spins about it, meets the floor half its thinnest size short
// of it, and settles onto its broad side where it rests (rest). Carried by
// the hull's origin, every piece of an AT-TE started on the ground, went
// straight up, struck the floor at once and lay in its standing place, two
// of them in the air.

const GRAVITY = -9.8;
const RESTITUTION = 0.37;
const FRICTION = 1.5755;
const SLOW = 1; // m/s
const REST_LIFE = 10; // s
const LIFT = 0.25; // share of the distance out added upward
const MAX_FLIGHT = 20; // s
const SINK = 0.15; // m a hull's piece lies in the floor
const SETTLE = 0.35; // s a hull's piece takes to settle where it rests
const AXES = [new THREE.Vector3(1, 0, 0), new THREE.Vector3(0, 1, 0), new THREE.Vector3(0, 0, 1)];
const UP = new THREE.Vector3(0, 1, 0);
const DOWN = new THREE.Vector3(0, -1, 0);
const _axis = new THREE.Vector3();
const _turned = new THREE.Quaternion();
const _box = new THREE.Box3();
const _middle = new THREE.Vector3();
const _size = new THREE.Vector3();

export class Debris {
  constructor({ scene, collision, effects, model }) {
    this.scene = scene;
    this.collision = collision;
    this.effects = effects;
    this.model = model;
    this.pieces = [];
    this.hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3() };
    this.ray = new THREE.Vector3();
    this.start = new THREE.Vector3();
    this.out = new THREE.Vector3();
  }

  // chunks: the class's CHUNKSECTIONs; bones: the body's bones by name (posed
  // and up to date); origin and velocity: the body's; health: at death (0 or
  // below); ignore: a collision body the pieces pass through (a destroyed
  // turret's wreck, which they start inside); centre: carry each piece by
  // its own middle (a hull's, above); floor: the height no piece starts
  // below (a hull's ground), its middle raised to half its height over it.
  blowApart(chunks, bones, origin, velocity, health, ignore = null, { centre = false, floor = -Infinity } = {}) {
    const f = health <= -50 ? 1 : Math.min(0.1 - (0.9 * health) / 50, 1);
    // Node names match whatever their case (the droideka's say "dummyroot").
    const byName = new Map(Object.entries(bones).map(([name, bone]) => [name.toLowerCase(), bone]));
    for (const chunk of chunks) {
      const make = chunk.ChunkGeometryName ? this.model(chunk.ChunkGeometryName) : null;
      const bone = byName.get(String(chunk.ChunkNodeName || "").toLowerCase());
      if (!make || !bone) continue;
      const mesh = make();
      let lift = 0, thin = 1;
      _size.set(0, 0, 0);
      if (centre) {
        mesh.updateMatrixWorld(true);
        _box.setFromObject(mesh);
        if (!_box.isEmpty()) {
          _box.getCenter(_middle);
          lift = Math.min(..._box.getSize(_size).toArray()) / 2;
          thin = _size.x <= _size.y && _size.x <= _size.z ? 0 : _size.y <= _size.z ? 1 : 2;
          for (const child of mesh.children) child.position.sub(_middle);
        } else _middle.set(0, 0, 0);
      }
      bone.updateWorldMatrix(true, false);
      bone.matrixWorld.decompose(mesh.position, mesh.quaternion, mesh.scale);
      if (centre) {
        mesh.position.copy(_middle.applyMatrix4(bone.matrixWorld));
        mesh.position.y = Math.max(mesh.position.y, floor + _size.y / 2);
      }
      mesh.scale.set(1, 1, 1);
      this.scene.add(mesh);
      const d = this.out.subVectors(mesh.position, origin);
      d.y += LIFT * d.length();
      if (d.lengthSq() < 1e-8) d.set(0, 1, 0);
      const still = String(chunk.ChunkPhysics || "").toUpperCase() === "STATIC";
      const v = still ? new THREE.Vector3() : d.normalize().multiplyScalar(chunk.ChunkSpeed ?? 0).add(velocity).multiplyScalar(f);
      const omega = Array.isArray(chunk.ChunkOmega) ? chunk.ChunkOmega : [0, 0, 0];
      const trailName = typeof chunk.ChunkTrailEffect === "string" ? chunk.ChunkTrailEffect : null;
      this.pieces.push({
        chunk,
        mesh,
        position: mesh.position,
        previous: mesh.position.clone(),
        velocity: v.clone(),
        omega: new THREE.Vector3(...omega),
        hits: 0,
        lift,
        thin,
        settle: null,
        ignore,
        resting: still,
        flight: 0,
        left: String(chunk.ChunkPhysics || "").toUpperCase() === "STATIC" ? Infinity : REST_LIFE,
        trail: trailName ? this.effects.play(trailName, mesh.position, null, v) : null,
        smoke: typeof chunk.ChunkSmokeEffect === "string" ? this.effects.play(chunk.ChunkSmokeEffect, mesh.position) : null,
      });
    }
  }

  update(dt) {
    for (const p of this.pieces) {
      if (p.resting) {
        p.left -= dt;
        const s = p.settle;
        if (s && s.t < SETTLE) {
          s.t = Math.min(s.t + dt, SETTLE);
          const w = s.t / SETTLE, e = w * w * (3 - 2 * w);
          p.mesh.quaternion.slerpQuaternions(s.from, s.to, e);
          p.position.y = s.y0 + (s.y1 - s.y0) * e;
          p.smoke?.move(p.position);
        }
        continue;
      }
      p.flight += dt;
      p.velocity.y += GRAVITY * dt;
      p.previous.copy(p.position);
      p.position.addScaledVector(p.velocity, dt);
      p.mesh.rotateX(p.omega.x * dt);
      p.mesh.rotateY(p.omega.y * dt);
      p.mesh.rotateZ(p.omega.z * dt);
      this.bounce(p);
      if (p.trail) p.trail.move(p.position, null, p.velocity);
      p.smoke?.move(p.position);
      if (p.flight > MAX_FLIGHT) this.rest(p);
    }
    for (const p of this.pieces) {
      if (p.left > 0) continue;
      this.scene.remove(p.mesh);
      if (p.trail) p.trail.stop();
      p.smoke?.stop();
      p.gone = true;
    }
    this.pieces = this.pieces.filter((p) => !p.gone);
  }

  bounce(p) {
    const ray = this.ray.subVectors(p.position, p.previous);
    const length = ray.length();
    if (length < 1e-6) return;
    ray.divideScalar(length);
    this.start.copy(p.previous).addScaledVector(ray, -0.05);
    // A piece carried by its middle meets the floor that far short of it.
    const hit = this.collision.cast(this.start, ray, length + 0.1 + p.lift, this.hit, p.ignore);
    if (!hit) return;
    const n = hit.normal;
    const v = p.velocity;
    const d = v.dot(n);
    if (d >= 0) return;
    p.position.copy(hit.point).addScaledVector(n, Math.max(0.05, p.lift));
    const most = p.chunk.ChunkTerrainCollisions ?? 0;
    if (p.hits < most && typeof p.chunk.ChunkTerrainEffect === "string") this.effects.play(p.chunk.ChunkTerrainEffect, hit.point);
    p.hits += 1;
    v.addScaledVector(n, -d);
    const along = v.length();
    if (along > 0) v.multiplyScalar(Math.max(along - Math.min(FRICTION * -d, along), 0) / along);
    v.addScaledVector(n, RESTITUTION * -d);
    if (p.hits >= most || v.length() < SLOW) this.rest(p, hit.point.y);
  }

  // floor: where it came to rest. A hull's piece comes to rest at whatever
  // turn it was spinning at, often on end (an AT-TE's 11 m flank stood up
  // like a post) or deep in the floor. It settles instead, over SETTLE s:
  // it tips onto its broad side (its thinnest size upright) and lies on the
  // floor, SINK into it.
  rest(p, floor = -Infinity) {
    if (p.lift > 0 && Number.isFinite(floor)) {
      const q = p.mesh.quaternion;
      const axis = _axis.copy(AXES[p.thin]).applyQuaternion(q);
      const target = new THREE.Quaternion().setFromUnitVectors(axis, axis.y < 0 ? DOWN : UP).multiply(q);
      const turned = _turned.copy(q);
      q.copy(target);
      p.mesh.updateMatrixWorld(true);
      const low = _box.setFromObject(p.mesh).min.y;
      q.copy(turned);
      p.settle = { from: turned.clone(), to: target, y0: p.position.y, y1: p.position.y + floor - SINK - low, t: 0 };
    }
    p.resting = true;
    p.velocity.set(0, 0, 0);
    if (p.trail) p.trail.stop();
    p.trail = null;
  }

  clear() {
    for (const p of this.pieces) {
      this.scene.remove(p.mesh);
      if (p.trail) p.trail.stop();
      p.smoke?.stop();
    }
    this.pieces = [];
  }
}
