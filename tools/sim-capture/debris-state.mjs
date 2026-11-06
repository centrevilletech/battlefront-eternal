// Snapshot codecs for page debris and portable replay fixtures. These
// describe visual pieces; they never drive an authoritative world tick.
import * as THREE from "three";

export class DebrisIds {
  constructor() {
    this.meshes = []; this.bodies = []; this.effects = []; this.meshMap = new Map(); this.bodyMap = new Map(); this.effectMap = new Map();
    this.free = { mesh: [], body: [], effect: [] };
  }
  key(kind, value) {
    if (value == null) return null;
    const values = this[{ mesh: "meshes", body: "bodies", effect: "effects" }[kind]], ids = this[`${kind}Map`];
    if (!ids.has(value)) { const id = this.free[kind].pop() ?? values.length; ids.set(value, id); values[id] = value; }
    return ids.get(value);
  }
  release(kind, kept) {
    const values = this[{ mesh: 'meshes', body: 'bodies', effect: 'effects' }[kind]], ids = this[`${kind}Map`];
    for (const [value, id] of ids) if (!kept.has(value)) { ids.delete(value); values[id] = null; this.free[kind].push(id); }
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
