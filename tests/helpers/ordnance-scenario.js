// A seeded battle's worth of rounds to draw, for comparing the ordnance
// presentation against recorded results. The scenario is plain data driven
// by a fixed generator: the same seed builds the same bolts, beams, rockets,
// grenades and replicated projectiles however it is run, so a golden
// recorded from the JavaScript this phase replaced (before it was deleted)
// still names what the kernel must draw.
//
// `adapter` is what draws: present(frame) draws the world's rounds and the
// extra draws among them; receive(event) hands the replicated projectile
// presentation an event.

export const DEFS = {
  rifle: { Velocity: 150, LifeSpan: 2, LaserLength: 3, LaserWidth: 0.1, GlowLength: 6, LaserTexture: "red", LaserGlowColor: [255, 40, 10, 255], LightColor: [255, 60, 20, 255], LightRadius: 4 },
  heavy: { Velocity: 120, LifeSpan: 2, LaserLength: 4, LaserWidth: 0.35, LaserTexture: "green", LaserGlowColor: [60, 255, 60, 200], LightRadius: 6 },
  sniper: { Velocity: 400, LifeSpan: 1, LaserLength: 8, LaserWidth: 0.08, FadeOutTime: 1, LaserTexture: "blue", LaserGlowColor: [120, 160, 255, 255], LightRadius: 3 },
  plain: { Velocity: 90, LifeSpan: 2, LaserWidth: 0, LightColor: [200, 200, 200, 255] },
  rocket: { base: "bullet", GeometryName: "rocket", TrailEffect: "trail", LightColor: [255, 160, 40, 255], LightRadius: 5, Velocity: 150, LifeSpan: 2 },
  shell: { base: "shell", GeometryName: "shell", Gravity: 1.5, LightRadius: 2, LightColor: [255, 255, 200, 255], Velocity: 60, LifeSpan: 4 },
  missile: { base: "missile", GeometryName: "missile", TrailEffect: "trail", Velocity: 35, LifeSpan: 3 },
  beam: { base: "fatray", LaserWidth: 0.6, LaserGlowColor: [255, 30, 30, 255], LightRadius: 0, LaserTexture: "beam" },
  fatbeam: { base: "fatray", LaserGlowColor: [30, 255, 255, 255], LightRadius: 7 },
  grenade: { base: "sticky", GeometryName: "grenade" },
  dumb: { base: "bullet", Velocity: 20 },
};

// Park-Miller: the same numbers in every run.
function rng(seed) {
  let s = seed % 2147483647;
  if (s <= 0) s += 2147483646;
  return () => (s = (s * 16807) % 2147483647) / 2147483647;
}

export function scenario(THREE, { seed = 1, frames = 90, bolts = 12 } = {}) {
  const r = rng(seed);
  const between = (a, b) => a + (b - a) * r();
  const pick = (list) => list[Math.floor(r() * list.length)];
  const V = (x = 0, y = 0, z = 0) => new THREE.Vector3(x, y, z);
  const unit = () => V(between(-1, 1), between(-1, 1), between(-1, 1)).normalize();
  const point = () => V(between(-30, 30), between(0, 10), between(-30, 30));
  const defs = Object.fromEntries(Object.entries(DEFS).map(([k, v]) => [k, { ...v }]));
  const mesh = () => ({ position: V(), quaternion: new THREE.Quaternion(0, 0, 0, 1) });
  const trail = () => ({ moves: [], move(p, q, v) { this.moves.push([p.toArray(), q ? q.toArray() : null, v ? v.toArray() : null]); } });
  const boltDefs = ["rifle", "heavy", "sniper", "plain"];

  // World bolts: stepped each tick; drawn between steps at alpha.
  const world = [];
  for (let i = 0; i < bolts; i++) {
    const def = defs[pick(boltDefs)], direction = unit();
    const speed = def.Velocity, life = def.LifeSpan * between(0.1, 1);
    world.push({
      id: "w" + i, start: Math.floor(between(0, frames * 0.6)), kind: "world", def, position: point(), direction, speed, life,
      flown: 0, drawn: 0, offset: r() < 0.4 ? V(between(-0.5, 0.5), between(-0.5, 0.5), between(-0.5, 0.5)) : null,
      merge: between(2, 9), muzzle: r() < 0.5 ? (r() < 0.15 ? V(500, 0, 0) : V(between(-1, 1), between(-1, 1), between(-1, 1))) : null,
      ended: false,
    });
  }
  // Replicated bolts, flown on the playout clock.
  const linear = [];
  for (let i = 0; i < Math.ceil(bolts / 2); i++) {
    const def = defs[pick(boltDefs)], speed = def.Velocity, life = def.LifeSpan * between(0.1, 1);
    linear.push({
      id: "l" + i, start: Math.floor(between(0, frames * 0.6)), def, position: point(), direction: unit(), speed, life, flown: 0, drawn: 0,
      offset: V(between(-0.4, 0.4), between(-0.4, 0.4), between(-0.4, 0.4)), merge: Math.max(Math.min(8, speed * life), 1e-3),
      networkFlightTick: Math.floor(between(0, frames * 0.6)), muzzle: r() < 0.6 ? V(between(-1, 1), between(0, 2), between(-1, 1)) : null, started: false,
    });
  }
  // The player's predicted shots.
  const predicted = [];
  for (let i = 0; i < Math.ceil(bolts / 3); i++) {
    const def = defs[pick(boltDefs)], speed = def.Velocity, life = Math.min(def.LifeSpan, between(0.05, 0.5));
    predicted.push({
      id: "p" + i, start: Math.floor(between(0, frames * 0.6)), def, origin: point(), direction: unit(), speed, life, flown: 0, drawn: 0,
      offset: r() < 0.5 ? V(between(-0.4, 0.4), between(-0.4, 0.4), between(-0.4, 0.4)) : null, merge: Math.max(Math.min(8, speed * life), 1e-3), age: 0,
    });
  }
  const rounds = [];
  for (let i = 0; i < 6; i++) {
    const def = defs[pick(["rocket", "shell", "missile", "dumb"])], from = point(), velocity = unit().multiplyScalar(def.Velocity ?? 50);
    rounds.push({
      id: "r" + i, start: Math.floor(between(0, frames * 0.6)), def, position: from.clone(), previous: from.clone(), launchPosition: r() < 0.7 ? from.clone() : null,
      velocity, quaternion: r() < 0.8 ? new THREE.Quaternion().setFromAxisAngle(unit(), between(0, 6)) : null,
      mesh: def.GeometryName ? mesh() : null, trail: def.TrailEffect ? trail() : null, muzzle: r() < 0.5 ? V(between(-2, 2), between(-2, 2), between(-2, 2)) : null, muzzleOffset: null,
    });
  }
  for (const x of [...world, ...linear, ...rounds]) x.visualMuzzle = x.muzzle ? () => x.muzzle : null;
  const beams = [];
  for (let i = 0; i < 3; i++) {
    const def = defs[pick(["beam", "fatbeam"])], position = point();
    beams.push({ id: "b" + i, start: Math.floor(between(0, frames * 0.5)), def, position, end: position.clone().add(unit().multiplyScalar(between(5, 80))), direction: unit(), life: between(0.02, 0.5) });
  }
  const grenades = [];
  for (let i = 0; i < 3; i++) {
    const from = point();
    grenades.push({ id: "g" + i, start: Math.floor(between(0, frames * 0.5)), def: defs.grenade, mesh: mesh(), previous: from.clone(), position: from.clone().add(unit()), velocity: unit().multiplyScalar(5) });
  }
  // Replicated projectiles (events): launches and samples, then ends.
  const events = [];
  const net = [];
  for (let i = 0; i < 8; i++) {
    const def = defs[pick(["rocket", "shell", "missile", "beam", "dumb", "grenade"])];
    const type = def === defs.beam ? "beam" : def === defs.grenade ? "grenade" : "round";
    net.push({ id: 100 + i, generation: 1, def, type, start: Math.floor(between(0, frames * 0.4)), position: point(), velocity: unit().multiplyScalar(between(10, 80)), life: between(0.2, 2) });
  }
  const definition = (id) => Object.values(defs)[id];
  const definitionId = (def) => Object.values(defs).indexOf(def);
  for (const p of net) {
    for (let k = 0; k < 6; k++) {
      const tick = p.start + k * 6;
      const position = p.position.clone().addScaledVector(p.velocity, k * 0.1);
      events.push({ at: Math.max(0, tick - 8), event: {
        schema: 6, kind: "combat", id: p.id, generation: p.generation, tick, expiryTick: tick + 18, definitionId: definitionId(p.def), type: p.type,
        position: position.toArray(), velocity: p.velocity.clone().multiplyScalar(k % 4 === 3 ? 0.1 : 1).toArray(), quaternion: [0, 0, 0, 1], end: position.clone().addScaledVector(p.velocity, 0.2).toArray(),
        life: Math.max(0, p.life - k * 0.1),
      } });
    }
    if (p.id % 2) events.push({ at: p.start + 20, event: { schema: p.id % 3 ? 2 : 7, kind: "combat", id: p.id, generation: p.generation, tick: p.start + 34, position: p.position.toArray() } });
  }
  events.sort((a, b) => a.at - b.at);

  const live = (item, frame) => frame >= item.start;
  const frame = (k) => {
    const world_ = [], linear_ = [], predicted_ = [], rounds_ = [], beams_ = [], grenades_ = [];
    for (const b of world) if (live(b, k) && !b.gone) {
      b.previousFlown = b.flown; b.previousDrawn = b.drawn;
      b.flown = Math.min(b.flown + b.speed / 60, b.speed * b.life);
      b.drawn = Math.min(b.flown, Math.max(b.def.LaserLength || 1, b.def.GlowLength || 0));
      b.ended = b.flown >= b.speed * b.life; b.fade = b.ended ? Math.max(0, 1 - (k - b.start) / 90) : undefined;
      if (b.ended && !b.def.FadeOutTime) b.gone = true;
      world_.push(b);
    }
    for (const b of linear) if (live(b, k)) linear_.push(b);
    for (const p of predicted) if (live(p, k)) {
      p.age = (k - p.start) / 60;
      const bolt = p.bolt ||= { id: p.id, def: p.def, position: p.origin.clone(), direction: p.direction, speed: p.speed, life: p.life, flown: 0, drawn: 0, offset: p.offset, merge: p.merge };
      const fade = p.def.FadeOutTime > 0 ? Math.max(0, 1 - Math.max(0, p.age - p.life) / p.def.FadeOutTime) : p.age < p.life ? 1 : 0;
      if (fade > 0) {
        bolt.flown = Math.min(p.age, p.life) * p.speed;
        bolt.drawn = Math.min(bolt.flown, Math.max(p.def.LaserLength || 1, p.def.GlowLength || 0, p.def.BlurLength || 0));
        bolt.ended = p.age >= p.life; bolt.fade = fade;
        bolt.position.copy(p.origin).addScaledVector(p.direction, bolt.flown);
        predicted_.push(bolt);
      }
    }
    for (const x of rounds) if (live(x, k)) {
      x.previous.copy(x.position); x.position.addScaledVector(x.velocity, 1 / 60);
      rounds_.push(x);
    }
    for (const b of beams) if (live(b, k) && k - b.start < 40) { b.life = Math.max(0, b.life - 1 / 60); beams_.push(b); }
    for (const g of grenades) if (live(g, k)) { g.previous.copy(g.position); g.position.addScaledVector(g.velocity, 1 / 60); grenades_.push(g); }
    return { frame: k, alpha: [0.25, 0.5, 0.75, 1][k % 4], clock: k + 0.5, world: world_, linear: linear_, predicted: predicted_, rounds: rounds_, beams: beams_, grenades: grenades_ };
  };
  return { defs, frame, events, definition, frames, mesh, trail, V };
}

// ---- running a scenario and reducing what it drew ----

const FNV = 2166136261;
function fnv(hash, bytes) {
  for (let i = 0; i < bytes.length; i++) hash = Math.imul(hash ^ bytes[i], 16777619) >>> 0;
  return hash;
}
const bytes = (view) => new Uint8Array(view.buffer, view.byteOffset, view.byteLength);
const doubles = (list) => bytes(Float64Array.from(list.map((n) => (n === undefined ? NaN : n))));

// What a frame drew, as hashes: each batch's quads (by texture name), the
// lights asked for, the sniper rounds handed to the trace, and where every
// mesh and trail ended up. `full` keeps the numbers for a failure's report.
export function digest(ordnance, capture, rounds, full = false) {
  const out = { batches: {}, lights: 0, touches: 0, objects: 0, counts: {} };
  // A batch is named by its texture, blend and profile (its key, less the
  // texture's random id).
  const batches = [...ordnance.renderer.batches.entries()].map(([key, batch]) => {
    const texture = batch.material?.uniforms?.map?.value;
    return { name: batch.mesh.name === "blaster-bolts" ? "blaster" : `${texture?.name}:${key.slice(key.indexOf(":") + 1)}`, batch };
  }).sort((a, b) => a.name.localeCompare(b.name));
  for (const { name, batch } of batches) {
    // A batch with nothing in it draws nothing; when one is first made is not drawing.
    if (!batch.count) continue;
    let hash = FNV;
    for (const key of Object.keys(batch.arrays)) {
      const size = batch.arrays[key].length / batch.capacity;
      hash = fnv(hash, bytes(batch.arrays[key].subarray(0, batch.count * size)));
    }
    out.batches[name] = [batch.count, hash];
    if (full) (out.full ||= {})[name] = Object.fromEntries(Object.entries(batch.arrays).map(([k, a]) => [k, Array.from(a.subarray(0, batch.count * a.length / batch.capacity))]));
  }
  out.lights = fnv(FNV, doubles(capture.lights.flat()));
  out.counts.lights = capture.lights.length;
  out.touches = fnv(FNV, doubles(capture.touches.flat()));
  out.counts.touches = capture.touches.length;
  const objects = [];
  for (const o of rounds) {
    if (o.mesh) objects.push(...o.mesh.position.toArray(), ...o.mesh.quaternion.toArray());
    if (o.trail) for (const m of o.trail.moves) objects.push(...m[0], ...(m[1] ?? [NaN, NaN, NaN, NaN]), ...(m[2] ?? [NaN, NaN, NaN]));
    if (o.muzzleOffset) objects.push(...o.muzzleOffset.toArray());
    // A scenario bolt's offset is the drawn muzzle the draw stored on it; a
    // replicated projectile's is its own correction, which shows in where its mesh is.
    if (o.offset && typeof o.id === "string") objects.push(...o.offset.toArray());
  }
  out.objects = fnv(FNV, doubles(objects));
  if (full) out.fullLights = capture.lights.slice();
  capture.lights.length = 0;
  capture.touches.length = 0;
  for (const o of rounds) if (o.trail) o.trail.moves.length = 0;
  return out;
}

// The stand-ins around an ordnance: a light pool and a sniper trace that
// only record, textures by name, and a mesh factory.
export function surroundings(THREE) {
  const capture = { lights: [], touches: [] };
  const textures = new Map();
  const texture = (name) => {
    if (!textures.has(name)) {
      const tex = new THREE.DataTexture(new Uint8Array([255, 80, 20, 255, 240, 60, 10, 255, 200, 60, 20, 255, 255, 120, 40, 255]), 2, 2);
      tex.name = name;
      textures.set(name, tex);
    }
    return textures.get(name);
  };
  // LightPool.add, which refuses a request with no reach.
  const lights = { add(position, rgb, radius, scale = 1) { if (!rgb || radius <= 0) return; capture.lights.push([position.x, position.y, position.z, rgb[0], rgb[1], rgb[2], radius, scale]); } };
  const trace = {
    touch(source, def, start, head, direction, ended, fade, origin) {
      capture.touches.push([source.id.charCodeAt(0) * 1000 + Number(source.id.slice(1)), ...start.toArray(), ...head.toArray(), ...direction.toArray(), +ended, fade, ...origin.toArray()]);
    },
  };
  return { capture, texture, lights, trace };
}

// Runs `frames` of a scenario through a driver, returning each frame's digest.
// driver: { ordnance, capture, present(frame), receive(event), objects() }.
export function run(THREE, driver, { seed = 1, frames = 90, bolts = 12, full = false } = {}) {
  const s = scenario(THREE, { seed, frames, bolts });
  const out = [];
  let next = 0;
  driver.attach(s);
  for (let k = 0; k < frames; k++) {
    while (next < s.events.length && s.events[next].at <= k) driver.receive(s.events[next++].event);
    const f = s.frame(k);
    driver.present(f);
    out.push(digest(driver.ordnance, driver.capture, driver.objects(f), full));
  }
  return out;
}
