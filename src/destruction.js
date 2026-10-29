import * as THREE from "three";
import { emitSound } from "./audio/events.js";
import { DebrisPieces, heatMaterial } from "./destruction-debris.js";
import { CHIP, DestructionFx, EMBER, FIRE, FLASH, GLOW, SCORCH, SMOKE, SPARK } from "./destruction-fx.js";

// Enhanced destruction effects (Graphics > Destruction effects): how droids
// die and how wrecked power-up droids smoulder, drawn in this port's own
// style. Classic leaves all of it to the 2004 effects and src/debris.js.
//
// Everything here is presentation. The simulation decides as it always has
// whether a droid blows apart (its ChunkFrequency roll) or plays a death
// clip, sets off the class's explosion (damage, push, sound, the AI hearing
// it), and keeps the body's hit shapes until the body is removed. This
// module only reads bodies and settings and draws: it never touches health,
// randomness the simulation uses, collision, or anything a headless or worker
// world runs (it isn't built there).
//
// A droid dies one of two ways on screen:
//
// - Blown apart by the simulation (or a droideka at the end of its death
//   clip): at once, a blast where its pieces are. A blue-white flash and its
//   light on the floor, a compact fireball turning to oily smoke, sparks that
//   skip off the floor, flakes of plating, a ring of dust thrown out along
//   the ground, a scorch mark, and arcs leaping from the body. Its pieces fly
//   from where its body parts were (see "pieces" below).
// - Killed on its feet (the 40% the simulation gives a death clip): it
//   shorts out first. Arcs crawl between its joints and sparks spit from
//   them for a moment while it starts to fall, then it blows apart the same
//   way, with its explosion's sound. A droideka shorts out for the whole of
//   its death clip, small pops taking the place of the stock blasts.
//
// Pieces are rigid bodies (src/destruction-debris.js) launched out from the
// blast: each one hot where it tore away, trailing smoke while it flies,
// crackling with arcs for a second or two, then smouldering where it lies,
// lit and shadowed like the world, until it crumbles away. A droid's blaster
// is thrown clear too.
//
// The droideka's five pieces all hang from its root node and are modelled
// in its standing pose; laid on that node they rebuild a standing droideka
// at whatever pose it died in. Each is matched instead to the bone whose
// part of the body it covers, and carried there.
//
// A power-up droid (ammo or health) destroyed goes up in a blast of its own
// and then smoulders as a wreck: a wind-drifted column of smoke lit by the
// sun, small flames and a flickering glow at first, embers, and now and then
// a crackle of sparks and arcs. It dies down over a couple of minutes, fades
// as a repair weapon rebuilds it, and stops when it is built again.

const SHORT_OUT = 0.42; // s a droid killed on its feet crackles before it blows
const CHARGE_TIME = 0.9; // s for a piece's leftover charge to fall by e
const SITE_LIFE = 180; // s a wreck smoulders unrepaired
const SITE_REACH = 160; // m: wrecks farther from the camera emit nothing
const ELECTRIC = [0.5, 0.72, 1.0];
const FIRELIGHT = [1.0, 0.62, 0.3];
const EMBERLIGHT = [1.0, 0.42, 0.12];
const NO_FLOOR = -1e4; // the height of "no floor": nothing skips off it
const UP = new THREE.Vector3(0, 1, 0);
const DOWN = new THREE.Vector3(0, -1, 0);

export class Destruction {
  // model(name): a function making that model (game.js flyingModel).
  // lightAt(position, out): the light a soldier there gets (game.js).
  constructor({ scene, collision, lights, settings, model, lightAt = null, random = Math.random }) {
    this.scene = scene;
    this.settings = settings;
    this.collision = collision;
    this.lights = lights;
    this.random = random;
    this.fx = new DestructionFx(scene);
    this.pieces = new DebrisPieces({ scene, collision, model, lightAt });
    this.deaths = []; // droids shorting out
    this.sites = []; // smouldering wrecks
    this.stations = [];
    this.stationStates = new Map();
    this.quiet = new WeakMap(); // explosion -> the same without its effect
    this.skeletons = new WeakMap(); // body geometry -> indices of the bones that carry it, and joints
    this.matches = new WeakMap(); // body geometry -> chunk name -> bone and offset
    this.poses = new WeakMap(); // body geometry -> its bind pose (bindPose)
    this.frames = []; // chunk frames for one burst
    this.blasts = 0;
    this.hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: null };
    this.v = Array.from({ length: 8 }, () => new THREE.Vector3());
    this.m = new THREE.Matrix4();
    this.center = new THREE.Vector3();
    this.eye = new THREE.Vector3();
    this.unlisten = settings?.onChange?.((key, value) => {
      if (key === "destructionEffects" && value !== "enhanced") this.clear();
    });
  }

  get enhanced() {
    return (this.settings?.get("destructionEffects") ?? "enhanced") === "enhanced";
  }

  rand(lo, hi) {
    return lo + (hi - lo) * this.random();
  }

  // A random direction, its up component scaled by `lift` and offset by `rise`.
  direction(out, lift = 1, rise = 0) {
    out.randomDirection();
    out.y = out.y * lift + rise;
    return out.normalize();
  }

  // The explosion a droid's death sets off in the simulation. Enhanced
  // draws its own blast, so the stock sprites are left out; damage, push,
  // sound and light are untouched.
  explosion(def) {
    if (!def || !this.enhanced) return def;
    let quiet = this.quiet.get(def);
    if (!quiet) this.quiet.set(def, (quiet = { ...def, Effect: null }));
    return quiet;
  }

  // Whether a unit is a droid that comes apart.
  chunked(unit) {
    return Boolean(unit?.classDef?.chunks?.length && unit.actor);
  }

  // The simulation blew a droid apart (game.js onBlowApart). Returns whether
  // Enhanced drew it; if not, the classic pieces are thrown as before.
  blowApart(unit) {
    if (!this.enhanced || !this.chunked(unit)) return false;
    const i = this.deaths.findIndex((death) => death.unit === unit);
    if (i >= 0) this.deaths.splice(i, 1);
    this.burst(unit, false);
    return true;
  }

  // A droid died (game.js onDeath, after any blow-apart). One still standing
  // (its death clip playing) shorts out, then blows apart.
  died(unit) {
    if (!this.enhanced || !this.chunked(unit) || !unit.actor.root.visible) return;
    if (this.deaths.some((death) => death.unit === unit)) return;
    this.deaths.push({
      unit, generation: unit.generation, t: 0,
      // A droideka's clip runs to its explosion; the simulation ends it.
      length: unit.walker ? 6 : SHORT_OUT,
      arcs: 0, sparks: 0, flashes: 0, smoke: 0,
    });
  }

  // A droideka's blast during its death clip (every 0.4 s). Returns whether
  // Enhanced drew it.
  deathBlast(unit) {
    if (!this.enhanced || !unit?.actor) return false;
    const { bones } = this.skeleton(unit.actor);
    if (bones.length) this.pop(this.someBone(unit.actor, bones).getWorldPosition(this.v[0]), 1);
    return true;
  }

  // Power-up droids to watch for wrecking (LiveObjects). Each wreck's glowing
  // materials are made now, on a hidden stand-in, so graphics.warmup()
  // compiles them before play.
  watch(live) {
    this.stations = live?.stations || [];
    this.stationStates.clear();
    for (const station of this.stations) {
      this.stationStates.set(station, station.state);
      station.looks?.destroyed?.root?.traverse((mesh) => {
        if (mesh.isMesh) for (const material of [mesh.material].flat()) this.warm(mesh.geometry, heatMaterial(material));
      });
    }
  }

  // Each class's pieces (and its held weapons'), built once ahead so their
  // models, textures and programs are ready before the first droid dies.
  prepare(classes) {
    for (const def of classes || []) {
      if (!def?.chunks?.length) continue;
      for (const chunk of def.chunks) if (chunk.ChunkGeometryName) this.pieces.shape(chunk.ChunkGeometryName);
      for (const weapon of def.weapons || []) if (weapon.GeometryName) this.pieces.shape(weapon.GeometryName);
    }
  }

  // A hidden mesh drawing with `material`, one for each kind of program.
  warm(geometry, material) {
    this.warmed ||= new Map();
    const key = `${JSON.stringify(material.defines)}:${Boolean(material.map)}:${material.alphaTest}:${material.side}:${material.transparent}`;
    if (this.warmed.has(key)) return;
    const mesh = new THREE.Mesh(geometry, material);
    mesh.visible = false;
    mesh.frustumCulled = false;
    this.scene.add(mesh);
    this.warmed.set(key, mesh);
  }

  // Which bones of a body carry its vertices (where arcs can crawl), and
  // each one's nearest such ancestor (the limb between them), as indices
  // into actor.bones: { bones: [i], joints: [child, parent, child, ...] }.
  // Every body of a model shares its geometry, so this is kept per geometry
  // and always read through the dying body's own bones.
  skeleton(actor) {
    const geometry = actor.body?.geometry;
    let entry = geometry && this.skeletons.get(geometry);
    if (entry) return entry;
    const used = new Set();
    const skin = geometry?.attributes.skinIndex;
    if (skin) for (let i = 0; i < skin.count; i++) used.add(skin.getX(i));
    const bones = [];
    actor.bones.forEach((bone, i) => { if (used.has(i) && !/^(hp_|dummyroot)/i.test(bone.name)) bones.push(i); });
    const joints = [];
    for (const i of bones) {
      let parent = actor.bones[i].parent;
      while (parent?.isBone && !bones.includes(actor.bones.indexOf(parent))) parent = parent.parent;
      if (parent?.isBone) joints.push(i, actor.bones.indexOf(parent));
    }
    entry = { bones, joints };
    if (geometry) this.skeletons.set(geometry, entry);
    return entry;
  }

  // One of a body's own carrying bones (skeleton().bones), at random.
  someBone(actor, bones) {
    return actor.bones[bones[Math.floor(this.random() * bones.length)]];
  }

  // Each chunk's frame in the world: its node's, or for a node several
  // chunks share (the droideka's root), the frame of the bone it covers.
  chunkFrames(unit) {
    const frames = this.frames;
    frames.length = 0;
    const chunks = unit.classDef.chunks || [];
    const byName = new Map(unit.actor.bones.map((bone) => [bone.name.toLowerCase(), bone]));
    const shared = new Map();
    for (const chunk of chunks) {
      const key = String(chunk.ChunkNodeName || "").toLowerCase();
      shared.set(key, (shared.get(key) || 0) + 1);
    }
    for (const chunk of chunks) {
      const key = String(chunk.ChunkNodeName || "").toLowerCase();
      const node = byName.get(key);
      const name = chunk.ChunkGeometryName;
      if (!node || !name) continue;
      const matrix = new THREE.Matrix4();
      const match = shared.get(key) > 1 ? this.match(unit, name, node) : null;
      if (match) matrix.multiplyMatrices(match.bone.matrixWorld, match.offset);
      else matrix.copy(node.matrixWorld);
      frames.push({ chunk, name, matrix });
    }
    return frames;
  }

  // The bone whose part of the body a chunk covers, from the bind pose, and
  // the chunk's frame relative to that bone's.
  match(unit, name, node) {
    const actor = unit.actor;
    const bind = unit.animator?.bind;
    const geometry = actor.body?.geometry;
    if (!bind || !geometry) return null;
    let byChunk = this.matches.get(geometry);
    if (!byChunk) this.matches.set(geometry, (byChunk = new Map()));
    const key = `${name}:${node.name}`;
    if (byChunk.has(key)) {
      const known = byChunk.get(key);
      return known && { bone: actor.bones[known.index], offset: known.offset };
    }
    const shape = this.pieces.shape(name);
    const bones = actor.bones;
    const { rest, centres } = this.bindPose(actor, bind);
    const nodeIndex = bones.indexOf(node);
    const centre = (shape?.center || new THREE.Vector3()).clone().applyMatrix4(rest[nodeIndex]);
    let best = -1, nearest = Infinity;
    centres.forEach((at, i) => {
      const d = at ? at.distanceToSquared(centre) : Infinity;
      if (d < nearest) { nearest = d; best = i; }
    });
    const known = best >= 0 ? { index: best, offset: new THREE.Matrix4().copy(rest[best]).invert().multiply(rest[nodeIndex]) } : null;
    byChunk.set(key, known);
    return known && { bone: bones[known.index], offset: known.offset };
  }

  // A body's bind pose: each bone's frame relative to the body, and where
  // its vertices sit on average (null for a bone carrying too few to say).
  // Worked out once per body model.
  bindPose(actor, bind) {
    const geometry = actor.body.geometry;
    let pose = this.poses.get(geometry);
    if (pose) return pose;
    const bones = actor.bones;
    const rest = [];
    bones.forEach((bone, i) => {
      const local = new THREE.Matrix4().compose(bind[i].p, bind[i].q, new THREE.Vector3(1, 1, 1));
      const parent = bones.indexOf(bone.parent);
      rest[i] = parent >= 0 && parent < i ? new THREE.Matrix4().multiplyMatrices(rest[parent], local) : local;
    });
    const sums = bones.map(() => new THREE.Vector3()), counts = bones.map(() => 0);
    const position = geometry.attributes.position, skin = geometry.attributes.skinIndex, v = new THREE.Vector3();
    for (let i = 0; i < position.count; i++) {
      const b = skin.getX(i);
      if (b >= bones.length) continue;
      sums[b].add(v.fromBufferAttribute(position, i).applyMatrix4(rest[b]));
      counts[b]++;
    }
    pose = { rest, centres: sums.map((sum, i) => (counts[i] >= 12 ? sum.divideScalar(counts[i]) : null)) };
    this.poses.set(geometry, pose);
    return pose;
  }

  // The floor under a point (for sparks and chips to skip off): its height,
  // and in this.flat how level it is (normal.y). With none within 6 m (a
  // droid killed in the air, or over a drop) there is nothing to skip off or
  // glow on: NO_FLOOR, and flat 0.
  floor(at) {
    const from = this.v[7].copy(at).addScaledVector(UP, 0.3);
    const hit = this.collision.cast(from, DOWN, 6, this.hit);
    this.flat = hit ? hit.normal.y : 0;
    return hit ? hit.point.y : NO_FLOOR;
  }

  // A droid blowing apart: the blast, and its pieces flying.
  burst(unit, sound) {
    const actor = unit.actor;
    actor.root.updateMatrixWorld(true);
    const frames = this.chunkFrames(unit);
    const centre = this.center.set(0, 0, 0);
    let count = 0;
    for (const frame of frames) {
      const shape = this.pieces.shape(frame.name);
      if (!shape) continue;
      centre.add(this.v[0].copy(shape.center).applyMatrix4(frame.matrix));
      count++;
    }
    if (count) centre.divideScalar(count);
    else centre.copy(unit.position).addScaledVector(UP, unit.walker ? 0.8 : 0.9);
    let spread = 0;
    for (const frame of frames) spread = Math.max(spread, this.v[0].setFromMatrixPosition(frame.matrix).distanceTo(centre));
    const scale = unit.walker ? 1.45 : THREE.MathUtils.clamp(0.75 + spread * 0.45, 0.9, 1.25);
    this.blast(centre, scale, unit.velocity);
    const velocity = unit.velocity || this.v[1].set(0, 0, 0);
    for (const frame of frames) this.throw(frame.name, frame.matrix, centre, velocity, frame.chunk.ChunkSpeed ?? 7, 1);
    // The droid's blaster, thrown clear.
    const weapon = unit.combat?.arsenal?.current?.(0)?.def?.GeometryName;
    const held = weapon && unit.combat.held?.get(weapon);
    if (held) this.throw(weapon, held.group.matrixWorld, centre, velocity, 5, 0.25);
    // Its pieces take its place, for the rest of its life (Unit.goUp); a
    // droideka's body dissolves under the blast meanwhile.
    actor.root.visible = false;
    unit.goUp?.();
    if (sound && unit.classDef.explosion) emitSound("explosion", unit.classDef.explosion, centre.clone());
  }

  // One piece out of a blast at `centre`.
  throw(name, matrix, centre, inherited, speed, heat) {
    const shape = this.pieces.shape(name);
    if (!shape) return null;
    const com = this.v[2].copy(shape.center).applyMatrix4(matrix);
    const out = this.v[3].subVectors(com, centre);
    const d = out.length();
    if (d < 0.05) this.direction(out, 0.5, 0.3);
    else out.divideScalar(d);
    out.y += 0.35;
    out.add(this.direction(this.v[4], 1, 0).multiplyScalar(0.35)).normalize();
    const velocity = out.multiplyScalar(speed * this.rand(0.4, 0.7)).addScaledVector(inherited, 0.5).addScaledVector(UP, this.rand(0.4, 1.4));
    const spin = this.direction(this.v[5]).multiplyScalar(this.rand(4, 11));
    // It tore away on the side that faced the blast.
    const inverse = this.m.copy(matrix).invert();
    const local = this.v[6].copy(centre).applyMatrix4(inverse);
    let tear = null, nearest = Infinity;
    for (const p of shape.points) {
      const d2 = this.v[7].addVectors(p, shape.center).distanceToSquared(local);
      if (d2 < nearest) { nearest = d2; tear = p; }
    }
    const tearPoint = this.v[7].copy(tear || shape.points[0]).add(shape.center);
    const piece = this.pieces.spawn(name, matrix, { velocity, spin, tear: tearPoint, heat: heat * this.rand(0.85, 1), soot: heat > 0.5 ? 1 : 0.3, from: centre });
    if (piece) {
      piece.charge = heat > 0.5 ? this.rand(0.7, 1) : 0.2;
      piece.trail = 0;
      piece.shed = 0;
      piece.crackle = this.rand(0.02, 0.1);
      piece.wisp = 0;
    }
    return piece;
  }

  // The blast: a flash and its light on the floor, a compact fireball
  // turning to smoke, sparks, flakes of plating, a ring of dust thrown out
  // along the ground, a scorch mark, arcs, and the light.
  blast(at, scale = 1, inherited = null, electric = 1) {
    this.blasts++;
    const fx = this.fx;
    const floor = this.floor(at);
    const flat = this.flat;
    const height = at.y - floor;
    const p = this.v[0], v = this.v[1];
    const still = this.v[3].set(0, 0, 0);
    fx.hotParticle(FLASH, at, still, 0.07, 0.22 * scale, 0.75 * scale, 0, 1, 1.6, electric);
    fx.hotParticle(FLASH, at, still, 0.14, 0.2 * scale, 0.6 * scale, 0, 1, 1.3, 0, 0.015);
    if (flat > 0 && height < 2.5) fx.hotParticle(GLOW, at, still, 0.38, 1.1 * scale, 2.1 * scale, 0, 1, 0.85 * (1 - height / 2.5), floor);
    for (let i = 0; i < 11; i++) {
      p.copy(at).add(this.direction(this.v[2]).multiplyScalar(this.rand(0, 0.18) * scale));
      this.direction(v, 0.8, 0.3).multiplyScalar(this.rand(1.5, 3.6) * scale);
      if (inherited) v.addScaledVector(inherited, 0.4);
      fx.hotParticle(FIRE, p, v, this.rand(0.28, 0.5), this.rand(0.1, 0.15) * scale, this.rand(0.3, 0.46) * scale, -0.4, 5, 1, -1e4, this.rand(0, 0.04));
    }
    const sparks = Math.round(48 * scale);
    for (let i = 0; i < sparks; i++) {
      this.direction(v, 0.75, 0.3).multiplyScalar(this.rand(4, 14));
      fx.hotParticle(SPARK, at, v, this.rand(0.4, 1.2), this.rand(0.014, 0.024), 0.01, 1, 0.8, this.rand(0.8, 1.4), floor);
    }
    const chips = Math.round(22 * scale);
    for (let i = 0; i < chips; i++) {
      this.direction(v, 0.8, 0.45).multiplyScalar(this.rand(2.5, 7));
      fx.smokeParticle(CHIP, at, v, this.rand(1, 1.8), this.rand(0.02, 0.05), 0, 1, 0.35, 1, { spin: this.rand(-12, 12), floor });
    }
    for (let i = 0; i < 9; i++) {
      p.copy(at).add(this.direction(this.v[2]).multiplyScalar(this.rand(0, 0.25) * scale));
      this.direction(v, 0.6, 0.45).multiplyScalar(this.rand(0.6, 1.6) * scale).addScaledVector(UP, this.rand(0.3, 0.9));
      fx.smokeParticle(SMOKE, p, v, this.rand(2, 3.4), this.rand(0.22, 0.3) * scale, this.rand(0.62, 0.95) * scale, -0.06, 1.8, 0.72,
        { tone: this.rand(0.08, 0.18), heat: 1, wind: 0.8, spin: this.rand(-0.4, 0.4), delay: this.rand(0.04, 0.14) });
    }
    if (height < 1.6 && flat > 0.6) {
      // The shock throws a ring of dust out along the ground.
      for (let i = 0; i < 26; i++) {
        const angle = (i / 26 + this.rand(-0.015, 0.015)) * Math.PI * 2;
        p.set(at.x + Math.cos(angle) * 0.3 * scale, floor + 0.1, at.z + Math.sin(angle) * 0.3 * scale);
        v.set(Math.cos(angle), 0.06, Math.sin(angle)).multiplyScalar(this.rand(3.5, 6) * scale);
        fx.smokeParticle(SMOKE, p, v, this.rand(1, 1.6), 0.2 * scale, this.rand(0.6, 0.9) * scale, 0, 4.5, 0.26, { tone: 0.62, wind: 0.4, spin: this.rand(-0.5, 0.5) });
      }
      if (flat > 0.85) fx.smokeParticle(SCORCH, p.set(at.x, floor, at.z), still, 26, this.rand(0.75, 0.95) * scale, 0, 0, 1, 0.7, { floor });
    }
    if (electric > 0) {
      for (let i = 0; i < 6; i++) {
        this.direction(p, 0.7, 0.25).multiplyScalar(this.rand(0.5, 1.2) * scale).add(at);
        fx.arc(null, at, null, p, this.rand(0.06, 0.22), 0.012, 1);
      }
    }
    this.lights.flash(at, FIRELIGHT, 7 * scale, 0.4);
    if (electric > 0) this.lights.flash(at, ELECTRIC, 5 * scale, 0.12);
  }

  // A small pop: the droideka's blasts while it dies, a spark burst.
  pop(at, scale = 1) {
    const fx = this.fx, v = this.v[1], p = this.v[2];
    const floor = this.floor(at);
    fx.hotParticle(FLASH, at, v.set(0, 0, 0), 0.08, 0.15 * scale, 0.45 * scale, 0, 1, 1.5, 1);
    for (let i = 0; i < 4; i++) {
      this.direction(v, 0.7, 0.5).multiplyScalar(this.rand(0.8, 2));
      fx.hotParticle(FIRE, at, v, this.rand(0.24, 0.4), 0.08 * scale, 0.26 * scale, -0.6, 4, 1);
    }
    for (let i = 0; i < 24; i++) {
      this.direction(v, 0.7, 0.35).multiplyScalar(this.rand(3, 9));
      fx.hotParticle(SPARK, at, v, this.rand(0.3, 0.8), 0.016, 0.009, 1, 1, 1.2, floor);
    }
    for (let i = 0; i < 2; i++) {
      this.direction(v, 0.3, 0.8).multiplyScalar(0.6);
      fx.smokeParticle(SMOKE, at, v, this.rand(1.6, 2.4), 0.15 * scale, 0.6 * scale, -0.1, 1.4, 0.6, { tone: 0.12, heat: 0.8, delay: 0.05 });
    }
    for (let i = 0; i < 3; i++) {
      this.direction(p, 0.8, 0.2).multiplyScalar(this.rand(0.35, 0.8)).add(at);
      fx.arc(null, at, null, p, this.rand(0.07, 0.16), 0.016, 1.3);
    }
    this.lights.flash(at, ELECTRIC, 4.5 * scale, 0.14);
  }

  // A droid shorting out: arcs along its limbs, sparks off its joints, a
  // flickering blue light, until it blows.
  shortOut(death, dt) {
    const unit = death.unit;
    death.t += dt;
    const actor = unit.actor;
    if (unit.generation !== death.generation || !unit.combat?.dead || !actor.root.visible) return false;
    if (death.t >= death.length) {
      this.burst(unit, true);
      return false;
    }
    actor.root.updateMatrixWorld(true);
    const { bones, joints } = this.skeleton(actor);
    if (!bones.length) return true;
    const own = actor.bones;
    const fx = this.fx, a = this.v[0], b = this.v[1], v = this.v[2], centre = this.v[3].set(0, 0, 0);
    for (const i of bones) centre.add(b.setFromMatrixPosition(own[i].matrixWorld));
    centre.divideScalar(bones.length);
    const big = unit.walker ? 1.3 : 1;
    // Arcs: mostly along a limb between two joints, some leaping off it.
    death.arcs += dt * 42 * big;
    while (death.arcs >= 1) {
      death.arcs--;
      if (joints.length && this.random() < 0.7) {
        const k = 2 * Math.floor(this.random() * (joints.length / 2));
        fx.arc(own[joints[k]], this.direction(a).multiplyScalar(0.04), own[joints[k + 1]], this.direction(b).multiplyScalar(0.04), this.rand(0.07, 0.16), 0.02, 1.5);
      } else {
        const bone = this.someBone(actor, bones);
        bone.getWorldPosition(b).add(this.direction(v, 1, 0).multiplyScalar(this.rand(0.25, 0.6)));
        fx.arc(bone, a.set(0, 0, 0), null, b, this.rand(0.05, 0.12), 0.016, 1.3);
      }
    }
    death.sparks += dt * 70 * big;
    const floor = this.floor(this.v[6].copy(unit.position).addScaledVector(UP, 0.5));
    while (death.sparks >= 1) {
      death.sparks--;
      this.someBone(actor, bones).getWorldPosition(a);
      this.direction(v, 0.8, 0.3).multiplyScalar(this.rand(2, 6.5));
      fx.hotParticle(SPARK, a, v, this.rand(0.2, 0.55), 0.018, 0.01, 1, 1.2, 1.3, floor);
    }
    // Blue-white flashes where it shorts, and a flickering glow about it.
    death.flashes += dt * 18;
    while (death.flashes >= 1) {
      death.flashes--;
      this.someBone(actor, bones).getWorldPosition(a);
      fx.hotParticle(FLASH, a, v.set(0, 0, 0), 0.09, 0.12, 0.32 * big, 0, 1, 1.4, 1);
    }
    if (this.random() < dt * 22) fx.hotParticle(FLASH, centre, v.set(0, 0, 0), 0.06, 0.6 * big, 0.8 * big, 0, 1, this.rand(0.25, 0.5), 1);
    death.smoke += dt * (unit.walker ? 6 : 3);
    while (death.smoke >= 1) {
      death.smoke--;
      this.someBone(actor, bones).getWorldPosition(a);
      fx.smokeParticle(SMOKE, a, v.set(this.rand(-0.2, 0.2), this.rand(0.4, 0.9), this.rand(-0.2, 0.2)), this.rand(1.4, 2.2), 0.08, 0.36 * big, -0.08, 1.2, 0.5, { tone: 0.1, heat: 0.2 });
    }
    this.lights.add(centre, ELECTRIC, 6 * big, 0.6 + 0.8 * this.random());
    return true;
  }

  // What the pieces give off: smoke and sparks as they fly, puffs of dust
  // where they land, arcs while charged, wisps of smoke while hot.
  emitPieces(dt) {
    const fx = this.fx, v = this.v[1], a = this.v[2], b = this.v[3];
    // Trails and wisps give way when many pieces are out at once.
    const room = fx.room(fx.smoke), sparkRoom = fx.room(fx.hot);
    for (const piece of this.pieces.pieces) {
      const body = piece.body, heat = piece.heat, at = body.position;
      if (piece.charge === undefined) continue;
      const ground = body.ground.found ? body.ground.point.y : NO_FLOOR;
      const flying = !body.sleeping && body.velocity.lengthSq() > 1.5;
      if (flying && heat > 0.12) {
        piece.trail += dt * 60 * heat * room;
        while (piece.trail >= 1) {
          piece.trail--;
          // Laid back along the path it flew this frame, so the trail is unbroken.
          a.copy(at).addScaledVector(body.velocity, -dt * this.random());
          v.copy(body.velocity).multiplyScalar(-0.08).add(this.direction(b).multiplyScalar(0.12));
          fx.smokeParticle(SMOKE, a, v, this.rand(0.6, 1.0), 0.03 + 0.02 * heat, this.rand(0.18, 0.3), -0.06, 2.4, 0.3 * heat,
            { tone: 0.22, heat: 0.5 * heat, wind: 0.6, spin: this.rand(-0.6, 0.6) });
        }
        piece.shed += dt * 14 * heat * sparkRoom;
        while (piece.shed >= 1) {
          piece.shed--;
          v.copy(body.velocity).multiplyScalar(0.3).add(this.direction(a).multiplyScalar(this.rand(1, 3)));
          fx.hotParticle(this.random() < 0.6 ? SPARK : EMBER, at, v, this.rand(0.25, 0.6), 0.014, 0.01, 0.8, 1.4, 0.9, ground);
        }
      } else if (heat > 0.06 && piece.rest < 12) {
        piece.wisp += dt * 3.2 * Math.min(heat * 2.5, 1) * room;
        while (piece.wisp >= 1) {
          piece.wisp--;
          a.copy(at).add(this.direction(b).multiplyScalar(piece.shape.radius * 0.4));
          v.set(this.rand(-0.1, 0.1), this.rand(0.35, 0.7), this.rand(-0.1, 0.1));
          fx.smokeParticle(SMOKE, a, v, this.rand(2.2, 3.4), 0.04, this.rand(0.22, 0.36), -0.05, 1, 0.24 * Math.min(heat * 3, 1),
            { tone: 0.22, heat: 0.4 * heat, wind: 1, spin: this.rand(-0.3, 0.3) });
          if (this.random() < 0.35) fx.hotParticle(EMBER, a, v.set(this.rand(-0.2, 0.2), this.rand(0.3, 0.8), this.rand(-0.2, 0.2)), this.rand(0.8, 1.6), 0.018, 0.012, -0.03, 1, 0.9);
        }
      }
      if (body.impact > 2.2) {
        const hard = Math.min(body.impact / 8, 1);
        const p = body.impactPoint;
        for (let i = 0; i < 2 + Math.round(hard * 3); i++) {
          this.direction(v, 0.4, 0.6).multiplyScalar(this.rand(0.4, 1.2));
          fx.smokeParticle(SMOKE, p, v, this.rand(0.7, 1.2), 0.06, this.rand(0.22, 0.38) * (0.6 + hard), 0.05, 3, 0.35 * hard + 0.1, { tone: 0.62, wind: 0.5 });
        }
        if (heat > 0.2) for (let i = 0; i < Math.round(4 + 8 * hard); i++) {
          this.direction(v, 0.5, 0.55).multiplyScalar(this.rand(1.5, 5) * hard + 1);
          fx.hotParticle(SPARK, p, v, this.rand(0.2, 0.5), 0.013, 0.008, 1, 1.5, 1, p.y - 0.01);
        }
      }
      body.impact = 0;
      piece.charge *= Math.exp(-dt / CHARGE_TIME);
      if (piece.charge > 0.08) {
        piece.crackle -= dt;
        if (piece.crackle <= 0) {
          piece.crackle = this.rand(0.04, 0.2) / piece.charge;
          const points = piece.shape.points, centre = piece.shape.center;
          a.copy(points[Math.floor(this.random() * points.length)]).add(centre);
          b.copy(points[Math.floor(this.random() * points.length)]).add(centre);
          fx.arc(piece.object, a, piece.object, b, this.rand(0.05, 0.12), 0.008, 0.85 * Math.min(piece.charge * 1.5, 1));
          if (this.random() < 0.5) {
            a.applyMatrix4(piece.object.matrixWorld);
            for (let i = 0; i < 4; i++) {
              this.direction(v, 0.8, 0.3).multiplyScalar(this.rand(1, 3.5));
              fx.hotParticle(SPARK, a, v, this.rand(0.15, 0.4), 0.012, 0.008, 1, 1.5, 1, ground);
            }
          }
        }
      }
    }
  }

  // Power-up droids wrecked since the last frame, and rebuilt ones.
  watchStations() {
    for (const station of this.stations) {
      const was = this.stationStates.get(station);
      const now = station.state;
      if (was === now) continue;
      this.stationStates.set(station, now);
      if (now === "destroyed" && this.enhanced) this.wreck(station);
    }
  }

  wreck(station) {
    const look = station.looks?.destroyed;
    const box = new THREE.Box3();
    const [lo, hi] = look?.model?.box || station.looks?.built?.model?.box || [[-0.5, 0, -0.5], [0.5, 1.2, 0.5]];
    box.set(new THREE.Vector3(...lo), new THREE.Vector3(...hi)).applyMatrix4(station.matrix);
    const centre = box.getCenter(new THREE.Vector3());
    const size = box.getSize(new THREE.Vector3());
    const top = new THREE.Vector3(centre.x, box.max.y - size.y * 0.15, centre.z);
    const hot = Array.from({ length: 3 }, () => new THREE.Vector3(
      this.rand(box.min.x, box.max.x) * 0.6 + centre.x * 0.4, box.min.y + size.y * this.rand(0.35, 0.8), this.rand(box.min.z, box.max.z) * 0.6 + centre.z * 0.4));
    const blastAt = station.center(new THREE.Vector3());
    this.blast(blastAt, 1.35, null, 1);
    const site = { station, t: 0, box, centre, top, hot, size, smoke: 0, flame: 0, ember: 0, crackle: this.rand(0.3, 0.8), look: null, meshes: [] };
    // The wreck glows and chars where it burns.
    if (look?.root) {
      site.look = { heat: 0.85, tear: new THREE.Vector4(0, 0, 0, 0.01), base: 0.2, scale: 0.32, soot: 1, dissolve: 0, sun: 1, brightness: 1, darken: 0, lighten: 0 };
      look.root.traverse((mesh) => {
        if (!mesh.isMesh) return;
        const source = mesh.userData.unheated || mesh.material;
        const heated = Array.isArray(source) ? source.map((m) => this.heated(m)) : this.heated(source);
        mesh.userData.unheated = source;
        mesh.material = heated;
        mesh.userData.heated = site.look;
        site.meshes.push(mesh);
      });
      const first = site.meshes[0]?.userData.unheated;
      const u = (Array.isArray(first) ? first[0] : first)?.uniforms;
      if (u?.sunScale) {
        site.look.sun = u.sunScale.value;
        site.look.brightness = u.brightness.value;
        site.look.darken = u.ambientPull.value.x;
        site.look.lighten = u.ambientPull.value.y;
      }
    }
    this.sites = this.sites.filter((other) => other.station !== station);
    this.sites.push(site);
    return site;
  }

  heated(material) {
    return material ? heatMaterial(material) : material;
  }

  // The wreck's original materials back.
  cool(site) {
    for (const mesh of site.meshes) {
      if (mesh.userData.unheated) mesh.material = mesh.userData.unheated;
      delete mesh.userData.unheated;
      delete mesh.userData.heated;
    }
    site.meshes.length = 0;
  }

  // One wreck's smoulder for a frame. Returns false once it is out.
  smoulder(site, dt) {
    const station = site.station;
    site.t += dt;
    const t = site.t;
    if (station.state !== "destroyed" || t > SITE_LIFE) {
      this.cool(site);
      return false;
    }
    // A repair weapon rebuilding it smothers it as it goes.
    const repaired = THREE.MathUtils.clamp(station.health / Math.max(station.maxHealth, 1e-6), 0, 1);
    const left = Math.pow(1 - repaired, 0.7);
    const fade = 1 - THREE.MathUtils.smoothstep(t, SITE_LIFE * 0.55, SITE_LIFE);
    // Burning at first, then a small flame flickering on for a minute or so.
    const flames = (Math.exp(-t / 7) + 0.1 * (1 - THREE.MathUtils.smoothstep(t, 40, 90))) * left;
    const smoke = (0.4 + 0.6 * Math.exp(-t / 20)) * fade * left;
    const crackle = (0.2 + 0.8 * Math.exp(-t / 12)) * (1 - THREE.MathUtils.smoothstep(t, 60, 120)) * left;
    if (site.look) {
      site.look.heat = 0.85 * Math.exp(-t / 50) * left;
      site.look.soot = 1;
    }
    if (this.eye.distanceToSquared(site.centre) > SITE_REACH * SITE_REACH) return true;
    const fx = this.fx, a = this.v[0], v = this.v[1];
    site.smoke += dt * (1.6 + 5 * smoke) * Math.min(smoke * 3, 1);
    while (site.smoke >= 1) {
      site.smoke--;
      // Thick and black while it burns, a thinner grey wisp as it dies down.
      const young = Math.exp(-t / 25);
      a.copy(site.top);
      a.x += this.rand(-0.3, 0.3) * site.size.x * (0.4 + 0.6 * young);
      a.z += this.rand(-0.3, 0.3) * site.size.z * (0.4 + 0.6 * young);
      v.set(this.rand(-0.15, 0.15), this.rand(0.6, 1.15), this.rand(-0.15, 0.15));
      // Dark enough to read against pale stone even as it thins.
      fx.smokeParticle(SMOKE, a, v, this.rand(4.5, 7), this.rand(0.12, 0.2) + 0.1 * young, this.rand(0.7, 1.1) + 0.5 * young, -0.12, 0.55,
        (0.44 + 0.25 * young) * Math.min(smoke * 2.5, 1),
        { tone: 0.1 + 0.1 * (1 - young) + this.rand(0, 0.06), heat: 0.7 * flames, wind: 1, spin: this.rand(-0.25, 0.25) });
    }
    site.flame += dt * 14 * flames;
    while (site.flame >= 1) {
      site.flame--;
      const hot = site.hot[Math.floor(this.random() * site.hot.length)];
      a.copy(hot).add(this.direction(v).multiplyScalar(0.08));
      v.set(this.rand(-0.2, 0.2), this.rand(0.5, 1.1), this.rand(-0.2, 0.2));
      fx.hotParticle(FIRE, a, v, this.rand(0.3, 0.55), this.rand(0.06, 0.09), this.rand(0.18, 0.28) * (0.5 + flames * 0.5), -0.9, 2.2, 0.85);
    }
    site.ember += dt * (0.8 + 2.5 * flames) * Math.min(smoke * 2.5, 1);
    while (site.ember >= 1) {
      site.ember--;
      const hot = site.hot[Math.floor(this.random() * site.hot.length)];
      a.copy(hot).add(this.direction(v).multiplyScalar(this.rand(0, 0.25)));
      v.set(this.rand(-0.35, 0.35), this.rand(0.4, 1.2), this.rand(-0.35, 0.35));
      fx.hotParticle(EMBER, a, v, this.rand(1.2, 2.6), this.rand(0.012, 0.02), 0.008, -0.04, 0.6, this.rand(0.6, 1));
    }
    site.crackle -= dt;
    if (site.crackle <= 0 && crackle > 0.05) {
      site.crackle = this.rand(0.5, 2.6) / crackle;
      const box = site.box;
      a.set(this.rand(box.min.x, box.max.x), this.rand(box.min.y + site.size.y * 0.3, box.max.y), this.rand(box.min.z, box.max.z));
      const floor = box.min.y;
      for (let i = 0; i < 6 + Math.round(10 * crackle); i++) {
        this.direction(v, 0.8, 0.35).multiplyScalar(this.rand(1.5, 5.5));
        fx.hotParticle(SPARK, a, v, this.rand(0.25, 0.7), 0.014, 0.009, 1, 1.2, 1, floor);
      }
      fx.hotParticle(FLASH, a, v.set(0, 0, 0), 0.07, 0.06, 0.2, 0, 1, 1, 1);
      const b = this.v[2].set(this.rand(box.min.x, box.max.x), this.rand(box.min.y + site.size.y * 0.2, box.max.y), this.rand(box.min.z, box.max.z));
      fx.arc(null, a, null, b, this.rand(0.08, 0.18), 0.01, 0.9);
      this.lights.flash(a, ELECTRIC, 3.5, 0.1);
    }
    const flicker = 0.75 + 0.25 * Math.sin(t * 23.0 + site.hot[0].x) * Math.sin(t * 7.3);
    const glow = (0.15 + 0.85 * flames) * Math.exp(-t / 50) * left * flicker;
    if (glow > 0.03) this.lights.add(a.copy(site.centre).addScaledVector(UP, 0.4), EMBERLIGHT, 3.5 + 2 * flames, 0.45 * glow);
    return true;
  }

  update(dt, camera = null) {
    dt = THREE.MathUtils.clamp(dt, 0, 0.1);
    this.watchStations();
    if (!this.enhanced) return;
    if (camera) this.eye.copy(camera.position);
    if (this.deaths.length) {
      let kept = 0;
      for (const death of this.deaths) if (this.shortOut(death, dt)) this.deaths[kept++] = death;
      this.deaths.length = kept;
    }
    this.pieces.update(dt, camera?.position);
    this.emitPieces(dt);
    if (this.sites.length) {
      let kept = 0;
      for (const site of this.sites) if (this.smoulder(site, dt)) this.sites[kept++] = site;
      this.sites.length = kept;
    }
    this.fx.update(dt);
  }

  clear() {
    this.deaths.length = 0;
    for (const site of this.sites) this.cool(site);
    this.sites.length = 0;
    this.pieces.clear();
    this.fx.clear();
  }

  // Gone for good: out of the scene and off the settings.
  dispose() {
    if (typeof this.unlisten === "function") this.unlisten();
    this.clear();
    this.pieces.dispose();
    this.fx.dispose();
    for (const mesh of this.warmed?.values() || []) mesh.removeFromParent();
    this.warmed?.clear();
  }

  // What is out, for the stats and checks.
  stats() {
    return {
      pieces: this.pieces.count, hot: this.fx.hot.count, smoke: this.fx.smoke.count, arcs: this.fx.arcs.live,
      deaths: this.deaths.length, sites: this.sites.length, draws: this.fx.draws + this.pieces.pieces.reduce((n, p) => n + p.object.children.length, 0),
      blasts: this.blasts, rays: this.pieces.solver.rays,
    };
  }
}
