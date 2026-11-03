// The rig recorder, a tools/world-hash.mjs plugin: while a battle runs
// exactly as world-hash runs it, it records what the simulation's rigs and
// poses answer, for rust/sim/tests/rig_replay.rs to answer the same:
//   - every riderPoints, riderHitShapes and riderDropPoint call
//     (src/rider-pose.js), with the rider's skeleton and clip;
//   - each droideka's hidden gun rig as Droideka.arm builds it, every
//     sampleMuzzles that poses it (the droideka's state going in, every
//     barrel's place coming out, and every Nth call the whole rig), every
//     reset of its animator and every gunAt read off it;
//   - each hull's rig as the Vehicle constructor builds it (src/vehicles.js:
//     its nodes, muzzles, aimers, turret and rider nodes, aimedByPose and
//     every world matrix at rest), and every Nth aimTick the rig as posed
//     (each node's place, turn and scale, and its world matrix);
//   - the Math.sin, cos, atan2, asin and acos answers the droideka rigs
//     used (an oracle, until rust/v8math is linked).
//
//   BFE_CONTENT_DIR=<game-data> node tools/world-hash.mjs [battle options] \
//     --plugin tools/sim-rig/record.mjs --rig-out FILE [--rig-every N]
//
// It changes nothing in the battle: the hook's pass-through and the method
// wrappers call straight on, the oracle wrappers return Math's own values,
// and what is written is read off the objects without calling game code.
// Recordings hold the game's skeletons and clips: keep them with the
// game's files, never in a repository.
import fs from 'node:fs';
import { spawnSync } from 'node:child_process';
import { register } from 'node:module';
import { fileURLToPath } from 'node:url';
import { Writer, RECORD, MAGIC } from '../sim-capture/format.mjs';

register('./hooks.mjs', import.meta.url);
const THREE = await import('three');
const { Droideka, DroidekaAnimator } = await import('../../src/droideka.js');
const { Vehicle } = await import('../../src/vehicles.js');

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i < 0 ? fallback : argv[i + 1]; };
const outFile = opt('rig-out', null);
const every = Number(opt('rig-every', 13));
if (!outFile) throw Error('sim-rig: --rig-out FILE');

const writer = new Writer();
const fd = fs.openSync(outFile, 'w');
fs.writeSync(fd, MAGIC);
let records = 0;
const counts = {};
function write(name, value) {
  writer.record(RECORD.frame, currentTick, name, value);
  records++;
  counts[name] = (counts[name] ?? 0) + 1;
  if (writer.length > (8 << 20)) fs.writeSync(fd, writer.take());
}

let game = null, currentTick = 0;
const f64 = (list) => new Float64Array(list);
const V = (v) => [v.x, v.y, v.z];
const Q = (q) => [q.x, q.y, q.z, q.w];

// ---- The oracle: Math's answers inside the recorded droideka calls.
let oracleOn = 0;
const oracle = new Map();
const CODES = { sin: 1, cos: 2, atan2: 3, asin: 4, acos: 5, atan: 6, exp: 7 };
const bits64 = new Float64Array(1), bits32 = new Uint32Array(bits64.buffer);
const key = (x) => { bits64[0] = x; return `${bits32[0]}:${bits32[1]}`; };
for (const [name, code] of Object.entries(CODES)) {
  const base = Math[name];
  Math[name] = name === 'atan2'
    ? function atan2(a, b) { const r = base(a, b); if (oracleOn > 0) oracle.set(`${code}|${key(a)}|${key(b)}`, [code, a, b, r]); return r; }
    : function oracleWrapped(a) { const r = base(a); if (oracleOn > 0) oracle.set(`${code}|${key(a)}|0`, [code, a, 0, r]); return r; };
}
// Euler syncs (three's callback on every quaternion write) answer values
// nothing in a rig reads; the Rust works them out only when read.
const eulerFromMatrix = THREE.Euler.prototype.setFromRotationMatrix;
THREE.Euler.prototype.setFromRotationMatrix = function setFromRotationMatrix(...args) {
  const was = oracleOn;
  oracleOn = 0;
  try { return eulerFromMatrix.apply(this, args); } finally { oracleOn = was; }
};

// ---- Skeletons and clips, each written once.
const skeletons = new WeakMap(); // bones array -> key
const skeletonsWritten = new Map(); // key -> bind values (as decomposed here)
function writeSkeleton(key) {
  if (skeletonsWritten.has(key)) return skeletonsWritten.get(key);
  const body = game.factory.loaded.get(`body:${key}`);
  if (!body) throw Error(`sim-rig: no body ${key}`);
  const bones = body.def.bones.map((b) => ({ name: b.name, parent: b.parent, local: f64(b.local) }));
  // The bind pose as buildActor decomposes it.
  const bind = body.def.bones.map((b) => {
    const p = new THREE.Vector3(), q = new THREE.Quaternion(), s = new THREE.Vector3();
    new THREE.Matrix4().fromArray(b.local).decompose(p, q, s);
    return { p, q, s };
  });
  write('rig.skeleton', { key, bones, bind: f64(bind.flatMap(({ p, q, s }) => [...V(p), ...Q(q), ...V(s)])) });
  skeletonsWritten.set(key, bind);
  return bind;
}
function skeletonOfAnimator(animator) {
  const bones = animator.bones ?? animator.actor?.bones;
  let k = skeletons.get(bones);
  if (k === undefined) {
    const unit = game.factory.built.find((u) => u.animator === animator || u.actor?.bones === bones);
    k = unit ? `${unit.side}/${unit.classDef.model}` : null;
    skeletons.set(bones, k);
  }
  return k;
}
const clipIds = new WeakMap();
let nextClip = 0;
function clipId(clip) {
  if (!clip) return -1;
  let id = clipIds.get(clip);
  if (id === undefined) {
    id = nextClip++;
    clipIds.set(clip, id);
    write('rig.clip', { id, name: clip.name, fps: clip.fps, frames: clip.frames, rootMotion: clip.rootMotion, tracks: new Int32Array(clip.tracks), data: new Float32Array(clip.data) });
  }
  return id;
}

// ---- Riders.
const POINTS = ['pelvis', 'neck', 'head'], PAIRS = ['hips', 'knees', 'ankles', 'shoulders', 'elbows', 'hands'];
const pointsOf = (p) => [...POINTS.flatMap((k) => V(p[k])), ...PAIRS.flatMap((k) => (p[k] ? [...V(p[k][0]), ...V(p[k][1])] : [NaN, NaN, NaN, NaN, NaN, NaN])), p.arms ? 1 : 0,
  ...(p.root ? V(p.root) : [NaN, NaN, NaN]), ...(p.rootTurn ? Q(p.rootTurn) : [NaN, NaN, NaN, NaN])];
const TYPES = ['sphere', 'capsule', 'cylinder', 'box'], PARTS = ['body', 'head'];
const shapesOf = (shapes) => shapes.flatMap((s) => [TYPES.indexOf(s.type), PARTS.indexOf(s.part), ...V(s.center), s.radius, s.half === s.halfSize ? NaN : s.half, s.half === s.halfSize ? 1 : 0, ...V(s.halfSize), s.yaw]);

const hooks = {
  riderPoints(fn, animator, sample, out) {
    const key = game && animator?.bones && sample?.clip ? skeletonOfAnimator(animator) : null;
    if (!key) return fn(animator, sample, out);
    const bind = writeSkeleton(key);
    const bones = animator.bones;
    const at = bones.every((b, i) => b.quaternion.equals(bind[i].q) && b.position.equals(bind[i].p));
    const posed = at ? null : f64(bones.flatMap((b) => [...Q(b.quaternion), ...V(b.position)]));
    const before = pointsOf(out);
    const result = fn(animator, sample, out);
    write('rig.rider', { skeleton: key, clip: clipId(sample.clip), pitch: sample.pitch, turn: sample.turn, time: sample.time, posed,
      shoulders: Boolean(out.shoulders), root: Boolean(out.root), rootTurn: Boolean(out.rootTurn), before: f64(before), result: result === out, after: f64(pointsOf(out)) });
    return result;
  },
  riderHitShapes(fn, points, frame, scale, shapes) {
    if (!game) return fn(points, frame, scale, shapes);
    const before = shapesOf(shapes);
    const count = fn(points, frame, scale, shapes);
    write('rig.hitShapes', { points: f64(pointsOf(points)), frame: f64(frame.elements), scale, shapes: shapes.length, count, before: f64(before), after: f64(shapesOf(shapes)) });
    return count;
  },
  riderDropPoint(fn, points, out) {
    if (!game) return fn(points, out);
    const result = fn(points, out);
    write('rig.drop', { points: f64(pointsOf(points)), out: f64(V(result)) });
    return result;
  },
  hullBuilt(vehicle, template) { writeHull(vehicle, template); },
};
globalThis.__bfeRig = hooks;

// ---- Droidekas: the hidden gun rig.
const droidekas = new WeakMap();
let nextDroideka = 0;
const MODES = 14, CLIP_NAMES = ['idle', 'turnl', 'turnr', 'death01', null, 'idle_to_leftfoot', null, 'walk_leftfoot_rightfoot', 'walk_rightfoot_leftfoot', 'leftfoot_to_idle', 'rightfoot_to_idle', 'fold', 'roll', 'unfold'];
function droidekaId(d) {
  let id = droidekas.get(d);
  if (id !== undefined) return id;
  id = nextDroideka++;
  droidekas.set(d, id);
  // Armed as the battle loads, before the plugin has the game: its body's
  // bones as they stand then (the bind pose), named and parented.
  const bones = d.actor.bones;
  const skeleton = { names: bones.map((b) => b.name), parents: new Int32Array(bones.map((b) => bones.indexOf(b.parent))),
    bind: f64(bones.flatMap((b) => [...V(b.position), ...Q(b.quaternion), ...V(b.scale)])) };
  const guns = d.guns.map((gun, g) => ({ bone: bones.indexOf(gun), barrels: d.gunBarrels[g].map((b) => (b === gun ? null : f64(V(b.position)))) }));
  write('rig.droideka.arm', { id, class: d.def.class ?? null, skeleton, guns, rollCenter: f64(V(d.muzzleAnimator.rollCenter)), speed: d.speed,
    clips: new Int32Array(Array.from({ length: MODES }, (_, m) => (CLIP_NAMES[m] ? clipId(d.clips.get(CLIP_NAMES[m])) : -1))) });
  return id;
}
const stateOf = (d) => f64([d.mode, d.time, d.dead ? 1 : 0, d.legs, d.aimPitch, d.aimYaw, d.roll, d.lean, d.speed, ...V(d.position)]);
const samples = new WeakMap();
function rigState(d) {
  const a = d.muzzleAnimator;
  return { bones: f64(d.muzzleActor.bones.flatMap((b) => [...V(b.position), ...Q(b.quaternion), ...V(b.scale)])),
    animator: f64([a.mode, a.wentUpRolled ? 1 : 0, a.transition, a.rollOrigin, a.facing, a.legs]),
    root: f64([...V(d.muzzleActor.root.position), ...Q(d.muzzleActor.root.quaternion)]) };
}
const sampleMuzzles = Droideka.prototype.sampleMuzzles;
Droideka.prototype.sampleMuzzles = function recordedSampleMuzzles(dt) {
  if (!this.muzzleActor) return sampleMuzzles.call(this, dt);
  const id = droidekaId(this);
  const state = stateOf(this);
  const n = (samples.get(this) ?? 0) + 1;
  samples.set(this, n);
  const modeBefore = this.muzzleAnimator.mode;
  oracleOn++;
  try { sampleMuzzles.call(this, dt); } finally { oracleOn--; }
  const barrels = f64(this.physicalBarrels.flat().flatMap((b) => [b.matrixWorld.elements[12], b.matrixWorld.elements[13], b.matrixWorld.elements[14]]));
  const whole = n % every === 1 || modeBefore !== this.muzzleAnimator.mode || this.muzzleAnimator.transition > 0;
  write('rig.droideka.sample', { id, dt, state, barrels, ...(whole ? rigState(this) : {}) });
};
const resetAnimator = DroidekaAnimator.prototype.reset;
DroidekaAnimator.prototype.reset = function recordedReset(yaw) {
  if (this.d?.muzzleAnimator === this && droidekas.has(this.d)) write('rig.droideka.reset', { id: droidekas.get(this.d), yaw, roll: this.d.roll });
  return resetAnimator.call(this, yaw);
};
const gunAt = Droideka.prototype.gunAt;
Droideka.prototype.gunAt = function recordedGunAt(out, selected = this.lastGun) {
  const result = gunAt.call(this, out, selected);
  if (this.physicalBarrels && droidekas.has(this)) {
    write('rig.droideka.gun', { id: droidekas.get(this), index: selected?.index ?? -1, barrel: selected?.barrel ?? -1, result: result ? f64(V(result)) : null });
  }
  return result;
};

// ---- Hulls.
const hullIds = new WeakMap(), hullNodeLists = new WeakMap();
let nextHull = 0;
const num = (v, fallback = 0) => (Number.isFinite(parseFloat(v)) ? parseFloat(v) : fallback);
const str = (v) => (v ? String(v) : null);
const vec3 = (v) => { const a = v === undefined || v === null ? null : String(v).trim().split(/\s+/).map(Number); return a && a.length >= 3 && a.slice(0, 3).every(Number.isFinite) ? a.slice(0, 3) : [0, 0, 0]; };
// The rig's nodes in the order the constructor made them: the root, the
// bones, the static bone, the markers (by Object3D id); not the meshes.
function hullNodes(v) {
  const bones = new Set(v.bones);
  const statics = v.root.children.filter((c) => c.isBone && !bones.has(c));
  const markers = [];
  v.root.traverse((o) => { if (o !== v.root && !bones.has(o) && !statics.includes(o) && !o.isSkinnedMesh) markers.push(o); });
  markers.sort((a, b) => a.id - b.id);
  return [v.root, ...v.bones, ...statics, ...markers];
}
function writeHull(v, template) {
  const id = nextHull++;
  hullIds.set(v, id);
  const nodes = hullNodes(v);
  hullNodeLists.set(v, nodes);
  const ix = (o) => (o ? nodes.indexOf(o) : -1);
  const animated = new Set();
  for (const clip of template.clips?.values() ?? []) for (const j of clip.tracks || []) if (j >= 0) animated.add(j);
  const desc = {
    model: v.def.model, flying: v.flying, creature: v.healthType === 'animal',
    bones: template.rig.bones.map((b) => ({ name: b.name, parent: b.parent, local: f64(b.local) })),
    muzzleTips: [...(template.muzzleTips ?? new Map())].map(([name, tip]) => ({ name, tip: f64(tip) })),
    animated: new Int32Array([...animated].sort((a, b) => a - b)), hasClips: Boolean(template.clips?.size),
    seats: v.def.seats.map((seat) => {
      const s = seat.settings || {};
      return {
        turretNodeName: str(s.TurretNodeName), firePointName: str(s.FirePointName), pilotAnimation: str(s.Pilot9Pose || s.PilotAnimation),
        ninePose: Boolean(s.Pilot9Pose), pilotPosition: str(s.PilotPosition), riderNodeHash: str(s['11ce2025']), shootable: num(s['2b60e217'], 1), pivot: f64(vec3(s.f2aa4a90)),
        weapons: seat.weapons.map((w) => ({ firePointName: str(w.def.FirePointName), class: w.def.class,
          aimers: w.aimers.map((a) => ({ node: typeof a.node === 'string' ? a.node : str(a.node), firePointName: str(a.FirePointName), hierarchyLevel: num(a.HierarchyLevel, 0),
            barrels: (a.barrels || []).map((b) => ({ barrelNodeName: str(b.BarrelNodeName), firePointName: str(b.FirePointName), length: num(b.BarrelLength, 0),
              recoil: b.BarrelRecoil !== undefined ? num(b.BarrelRecoil, 0) : null })) })) })),
      };
    }),
  };
  const parent = (o) => (o.parent ? nodes.indexOf(o.parent) : -1);
  const kinds = nodes.map((o, i) => (i === 0 ? 0 : v.bones.includes(o) ? 1 : o.isBone ? 2 : 3));
  const rig = {
    kinds: new Int32Array(kinds), parents: new Int32Array(nodes.map(parent)),
    locals: f64(nodes.flatMap((o) => [...V(o.position), ...Q(o.quaternion), ...V(o.scale)])),
    worlds: f64(nodes.flatMap((o) => o.matrixWorld.elements)),
    seats: v.seats.map((seat) => ({
      turret: ix(seat.view.turret), pivot: f64(V(seat.view.pivot)), up: f64(V(seat.view.up)),
      rider: seat.view.rider ? { anim: seat.view.rider.anim, clip: seat.view.rider.clip, ninePose: seat.view.rider.ninePose, node: ix(seat.view.rider.node), shootable: seat.view.rider.shootable } : null,
      weapons: seat.weapons.map((w) => ({
        // The fire point is only in its beams' source: the node whose world
        // matrix that source's matrix getter gives.
        fire: w.emitter ? nodes.findIndex((o) => o.matrixWorld === w.emitter.matrix) : -1,
        aimers: w.aimers.map((a) => ({ bone: ix(a.bone), owner: a.owner })),
        muzzles: w.muzzles.map((m) => ({ fireNode: ix(m.fireNode), sourceAimer: m.sourceAimer ? w.aimers.indexOf(m.sourceAimer) : -1, barrel: ix(m.barrel ?? null),
          recoil: m.recoil ?? null, rest: m.rest ? f64(V(m.rest)) : null })),
      })),
    })),
    aimedByPose: v.aimedByPose,
  };
  write('rig.hull', { id, class: v.name, desc, rig });
}
const hullPoses = new WeakMap();
const aimTick = Vehicle.prototype.aimTick;
Vehicle.prototype.aimTick = function recordedAimTick(alpha, dt) {
  const result = aimTick.call(this, alpha, dt);
  if (hullIds.has(this) && this.alive) {
    const n = (hullPoses.get(this) ?? 0) + 1;
    hullPoses.set(this, n);
    if (n % every === 1) {
      const nodes = hullNodeLists.get(this);
      write('rig.hull.pose', { id: hullIds.get(this), locals: f64(nodes.flatMap((o) => [...V(o.position), ...Q(o.quaternion), ...V(o.scale)])),
        worlds: f64(nodes.flatMap((o) => o.matrixWorld.elements)) });
    }
  }
  return result;
};

// ---- The plugin's hooks.
export function battle({ game: g, index, options }) {
  if (index > 0) throw Error('sim-rig: one battle a recording (no --battles)');
  game = g;
  const root = fileURLToPath(new URL('../../', import.meta.url));
  const head = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).stdout.trim();
  const dirty = spawnSync('git', ['status', '--porcelain', '--', 'src', 'tools'], { cwd: root, encoding: 'utf8' }).stdout.trim();
  write('rig.header', { format: 1, tool: 'sim-rig', node: process.version, v8: process.versions.v8, commit: dirty ? `${head}+changes` : head, every,
    battle: { map: options.map, side: options.side, seed: options.seed, ticks: options.ticks, ai: options.ai } });
}
export function tick(t) { currentTick = t; }
export function finish() {
  write('rig.oracle', { table: f64([...oracle.values()].flat()) });
  fs.writeSync(fd, writer.take());
  fs.closeSync(fd);
  fs.writeFileSync(`${outFile}.json`, JSON.stringify({ records, counts, oracle: oracle.size, bytes: fs.statSync(outFile).size }, null, 1) + '\n');
}
