// Optional native physics for the page's Classic debris. Model factories,
// named bones, geometry bounds and effects stay on the rendering host.
// No authoritative world-step path calls this adapter.
import * as THREE from "three";
import { bridgeFor } from "./sim/runtime.js";
import { putDbInput, DbStateMirror, syncDbState } from "./sim/gen/props.js";

const ZERO = new THREE.Vector3();
export class DebrisIds {
  constructor() { this.meshes = []; this.bodies = []; this.effects = []; this.meshMap = new Map(); this.bodyMap = new Map(); this.effectMap = new Map(); }
  key(kind, value) {
    if (value == null) return null;
    const values = this[{ mesh: "meshes", body: "bodies", effect: "effects" }[kind]], ids = this[`${kind}Map`];
    if (!ids.has(value)) { ids.set(value, values.length); values.push(value); }
    return ids.get(value);
  }
}
// Text remains JavaScript text, including case conversion and empty names.
// ChunkOmega is the loader's numeric vector, with Vector3's defaults for
// absent entries. Raw object-valued class fields are not this boundary.
export function debrisChunk(c) {
  const omega = new THREE.Vector3(...(Array.isArray(c.ChunkOmega) ? c.ChunkOmega : []));
  if (![omega.x, omega.y, omega.z].every(n => typeof n === "number")) throw TypeError("debris requires a cooked numeric ChunkOmega");
  return { speed: Number(c.ChunkSpeed ?? 0), omega, still: String(c.ChunkPhysics || "").toUpperCase() === "STATIC",
    terrainCollisions: Number(c.ChunkTerrainCollisions ?? 0), terrainEffect: typeof c.ChunkTerrainEffect === "string" ? c.ChunkTerrainEffect : null,
    trail: typeof c.ChunkTrailEffect === "string" && c.ChunkTrailEffect ? c.ChunkTrailEffect : null,
    smoke: typeof c.ChunkSmokeEffect === "string" ? c.ChunkSmokeEffect : null };
}
export function debrisPiece(p, ids) {
  return { id: ids.key("mesh", p.mesh), chunk: debrisChunk(p.chunk), position: p.position, previous: p.previous, velocity: p.velocity,
    omega: p.omega, quaternion: p.mesh.quaternion, hits: p.hits, lift: p.lift, thin: p.thin, settle: p.settle,
    ignore: ids.key("body", p.ignore), resting: p.resting, flight: p.flight, left: p.left,
    trail: ids.key("effect", p.trail), smoke: ids.key("effect", p.smoke), gone: p.gone };
}
export function debrisState(d, ids) { return { pieces: d.pieces.map(p => debrisPiece(p, ids)), ray: d.ray, start: d.start, out: d.out }; }
export function attachDebrisSim(instance, debris) { return new DebrisSim(bridgeFor(instance), debris); }
export class DebrisSim {
  constructor(bridge, debris) {
    this.bridge = bridge; this.x = bridge.x; this.debris = debris; this.ids = new DebrisIds(); this.pieces = new Map(); this.chunks = new Map();
    this.input = { words: this.x.debris_words(0), put: putDbInput };
    this.output = { words: this.x.debris_words(1), Mirror: DbStateMirror, sync: syncDbState };
    bridge.register({ dbAction: a => this.action(a) });
    debris.blowApart = (...args) => this.blowApart(...args);
    debris.update = dt => this.run("update", { dt });
    debris.bounce = p => this.run("bounce", { id: this.ids.key("mesh", p.mesh) });
    debris.rest = (p, floor = -Infinity) => this.run("rest", { id: this.ids.key("mesh", p.mesh), floor });
    debris.clear = () => this.run("clear");
  }
  run(op, values = {}) {
    const d = this.debris;
    for (const p of d.pieces) { const id = this.ids.key("mesh", p.mesh); this.pieces.set(id, p); this.chunks.set(id, p.chunk); }
    this.bridge.callArgs(this.input, { op, state: debrisState(d, this.ids), launch: null, dt: 0, id: 0, floor: -Infinity, ...values });
    const result = this.bridge.callResult(this.output, this.x.debris_run());
    d.ray.copy(result.ray); d.start.copy(result.start); d.out.copy(result.out);
    const pieces = result.pieces.map(p => this.publish(p));
    if (op === "update" || op === "clear") d.pieces = pieces;
    else for (const p of pieces) if (!d.pieces.includes(p)) d.pieces.push(p);
  }
  publish(v) {
    let p = this.pieces.get(v.id);
    if (!p) {
      const mesh = this.ids.meshes[v.id];
      p = { chunk: this.chunks.get(v.id), mesh, position: mesh.position, previous: new THREE.Vector3(), velocity: new THREE.Vector3(), omega: new THREE.Vector3() };
      this.pieces.set(v.id, p);
    }
    for (const key of ["position", "previous", "velocity", "omega"]) p[key].copy(v[key]);
    p.mesh.quaternion.copy(v.quaternion);
    for (const key of ["hits", "lift", "thin", "resting", "flight", "left"]) p[key] = v[key];
    if (v.gone === undefined) delete p.gone; else p.gone = v.gone;
    p.ignore = v.ignore === null ? null : this.ids.bodies[v.ignore];
    p.trail = v.trail === null ? null : this.ids.effects[v.trail]; p.smoke = v.smoke === null ? null : this.ids.effects[v.smoke];
    if (v.settle) {
      p.settle ??= { from: new THREE.Quaternion(), to: new THREE.Quaternion() };
      p.settle.from.copy(v.settle.from); p.settle.to.copy(v.settle.to);
      for (const key of ["y0", "y1", "t"]) p.settle[key] = v.settle[key];
    } else p.settle = null;
    return p;
  }
  blowApart(chunks, bones, origin, velocity, health, ignore = null, { centre = false, floor = -Infinity } = {}) {
    const byName = new Map(Object.entries(bones).map(([name, bone]) => [name.toLowerCase(), bone]));
    for (const chunk of chunks) {
      const make = chunk.ChunkGeometryName ? this.debris.model(chunk.ChunkGeometryName) : null;
      const bone = byName.get(String(chunk.ChunkNodeName || "").toLowerCase());
      if (!make || !bone) continue;
      const mesh = make(), bounds = new THREE.Box3();
      if (centre) { mesh.updateMatrixWorld(true); bounds.setFromObject(mesh); }
      bone.updateWorldMatrix(true, false);
      const id = this.ids.key("mesh", mesh); this.chunks.set(id, chunk);
      this.run("launch", { launch: { id, chunk: debrisChunk(chunk), matrix: bone.matrixWorld, low: bounds.min, high: bounds.max,
        origin, velocity, health, ignore: this.ids.key("body", ignore), center: centre, floor } });
    }
  }
  action(a) {
    const d = this.debris, result = { hit: false, point: ZERO, normal: ZERO, low: 0, effect: null };
    // Read or copy every argument before invoking host code: another module
    // command may reuse this import's argument object and backing vectors.
    switch (a.kind) {
      case "publish": this.publish(a.piece); break;
      case "out": d.out.copy(a.point); break;
      case "vectors": d.start.copy(a.point); d.ray.copy(a.direction); break;
      case "add": {
        const mesh = this.ids.meshes[a.id], point = a.point.clone(), turn = a.turn.clone(), shift = a.shift?.clone();
        if (shift) for (const child of mesh.children) child.position.sub(shift);
        mesh.position.copy(point); mesh.quaternion.copy(turn); mesh.scale.set(1, 1, 1); d.scene.add(mesh); break;
      }
      case "remove": { const p = this.publish(a.piece); d.scene.remove(p.mesh); break; }
      case "cast": {
        const at = d.start.copy(a.point), ray = d.ray.copy(a.direction), far = a.far, ignore = a.ignore === null ? null : this.ids.bodies[a.ignore];
        const hit = d.collision.cast(at, ray, far, d.hit, ignore);
        this.lastHit = hit;
        if (hit) Object.assign(result, { hit: true, point: hit.point.clone(), normal: hit.normal.clone() }); break;
      }
      case "low": {
        const p = this.publish(a.piece), turn = a.turn.clone(), original = p.mesh.quaternion.clone();
        p.mesh.quaternion.copy(turn); p.mesh.updateMatrixWorld(true);
        result.low = new THREE.Box3().setFromObject(p.mesh).min.y;
        p.mesh.quaternion.copy(original); break;
      }
      case "play": {
        const name = a.name, point = a.impact ? this.lastHit.point : this.ids.meshes[a.piece.id].position;
        const velocity = a.velocity ? (a.piece.chunk.still ? a.velocity.clone() : d.out) : null;
        const effect = velocity ? d.effects.play(name, point, null, velocity) : d.effects.play(name, point);
        result.effect = this.ids.key("effect", effect); break;
      }
      case "move": {
        const effect = this.ids.effects[a.id], p = this.pieces.get(a.piece.id), point = p.position, velocity = a.velocity ? p.velocity : null;
        if (velocity) effect.move(point, null, velocity); else effect.move(point); break;
      }
      case "stop": { const effect = this.ids.effects[a.id]; effect.stop(); break; }
    }
    return result;
  }
}
