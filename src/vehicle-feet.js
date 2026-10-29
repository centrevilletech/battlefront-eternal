import * as THREE from "three";
import { Pose } from "./anim.js";
import { classSetting } from "./vehicle-motion.js";

// A walker's feet (docs/vehicles.md "Effects and sounds"): each
// TerrainCollision primitive of its class (tools/zero_vehicle.py vehicle_fx
// `feet`, `stomp`), and when each comes down and lifts, by the rules of the
// 2004 game's walker foot update:
//
// - A foot is where its primitive meets the ground (`bottom`: its centre
//   less its up axis times half its height), and its height is that
//   point's over the walker's root.
// - It comes down (the stomp: the StompEffect, the foot's FootstepSound) by
//   the class's StompDetectionType. 0, the default: as it falls through
//   StompThreshold (0.15 m unless set) over the lowest it goes. 1 (the
//   AT-AT, AT-TE and spider walker): once it falls less than 0.1 m in an
//   update after falling more. The update runs only while the walker walks
//   or turns, and standing clears type 1's state, so as it sets off every
//   foot of a type 1 walker stomps at once.
// - It lifts (HydraulicSound) as it rises past HydraulicSoundHeight (1 m
//   unless set), once a rise.
//
// The rules read a foot's travel per update, so they hang on the update
// rate, and the walk cycle plays at one pace in the 2004 game (its
// AnimationSpeed). The port reads them once per class at 60 updates a
// second over the walk clips (gait()), as the phases of the cycle at which
// each foot comes down and lifts, the fall measured over any 1/60 s. A
// frame's walk phase as drawn (Vehicle.poseCrew: his own hull's predicted
// cycle, any other's replicated one) passing one of those phases is the
// step, whatever the frame rate or the records' rate; the place is the
// foot as drawn. The stomp effect (src/vehicle-effects.js) and the
// footsteps and hydraulics (src/audio/vehicles.js) each keep their own
// Footfalls. A walker without a walk cycle falls back on the drawn foot's
// fall (fall()).

const UPDATE = 1 / 60; // s: the update the rules are read over (the port's tick)
const DESCENT = .1; // m an update: a type 1 foot's fall
const STEPS = 16; // points of the rules' grid an update
const WALKING = .01; // walk blend over which it walks (the original game's moving states)
const RULES = { detection: 0, threshold: .15, hydraulic: 1 }; // the class defaults
// fall(): a foot falling faster than DROP that then stops falling has come
// down; one that then rises as fast is lifting (m/s).
const DROP = .5;
const LANDED = .15;
const gaits = new WeakMap(); // class def -> its gait, or null
const _point = new THREE.Vector3();
const _turn = new THREE.Quaternion();
const _q = new THREE.Quaternion();

// Each foot's height over the root at each key of the walk cycle (pure, at
// full blend, idle out; the two clips' keys lie 1/fps apart all round),
// posed on a copy of the hull's skeleton so the drawn one is left alone.
// Between keys the pose blends linearly, and so (near enough) does a foot.
function cycleHeights(v, feet) {
  const root = new THREE.Object3D();
  const bones = v.bones.map((b) => { const bone = new THREE.Bone(); bone.scale.copy(b.scale); return bone; });
  v.bones.forEach((b, i) => { const parent = v.bones.indexOf(b.parent); (parent >= 0 ? bones[parent] : root).add(bones[i]); });
  const byName = Object.fromEntries(v.bones.map((b, i) => [b.name.toLowerCase(), bones[i]]));
  const nodes = feet.map((foot) => (foot.bone && byName[foot.bone]) || root);
  // Only the bones from the root to a foot move it: the clips' other tracks
  // are left out.
  const chain = new Set();
  for (const node of nodes) for (let n = node; n && n !== root; n = n.parent) chain.add(bones.indexOf(n));
  const only = (clip) => ({ ...clip, tracks: clip.tracks.map((bone) => (chain.has(bone) ? bone : -1)) });
  const clips = [only(v.clips.get("walk_leftfoot_rightfoot")), only(v.clips.get("walk_rightfoot_leftfoot"))];
  const keys = clips.flatMap((clip) => Array.from({ length: Math.max(clip.frames - 1, 0) }, (_, j) => [clip, j / clip.fps]));
  const pose = new Pose(bones.length, v.pose.root);
  const heights = feet.map(() => new Float32Array(keys.length));
  keys.forEach(([clip, time], k) => {
    for (let i = 0; i < bones.length; i++) { bones[i].quaternion.copy(v.bindQuaternions[i]); bones[i].position.copy(v.bindPositions[i]); }
    pose.clear();
    pose.add(clip, time, 1, false);
    pose.apply(bones);
    feet.forEach((foot, i) => {
      nodes[i].updateWorldMatrix(true, false);
      heights[i][k] = _point.fromArray(foot.bottom || foot.at).applyMatrix4(nodes[i].matrixWorld).y;
    });
  });
  return heights;
}

// When one foot comes down and lifts round a cycle of `length` seconds
// whose heights lie evenly spaced, walked forward (direction 1) or played
// backward (-1), each rule read over the update before (`rate`: the
// cycle's seconds a second) at every point of a fine grid (STEPS an
// update), so its fall over any 1/60 s counts: stomps, each with the phase
// its foot armed at (type 1: where it began to fall faster than DESCENT;
// null for type 0), and lifts. Read the second time round; the first
// settles the state each rule keeps.
export function footSchedule(heights, length, rules, rate = 1, direction = 1) {
  const count = heights.length, spacing = length / count, window = rate * UPDATE;
  const at = (phase) => {
    const f = ((phase % length + length) % length) / spacing, i = Math.floor(f) % count, a = f - Math.floor(f);
    return heights[i] + (heights[(i + 1) % count] - heights[i]) * a;
  };
  let low = Infinity;
  for (const h of heights) low = Math.min(low, h);
  const level = low + rules.threshold;
  // A lift is read from where the foot meets the ground; class data older
  // than its bottom (feet at their primitives' centres) from the lowest.
  const lift0 = rules.centred ? low : 0;
  const steps = Math.max(count, Math.ceil(length / window * STEPS)), step = length / steps;
  const stomps = [], lifts = [];
  let landed = true, armedAt = null, crossing = false, lifted = false;
  for (let pass = 0; pass < 2; pass++) {
    for (let k = 0; k < steps; k++) {
      const phase = direction > 0 ? k * step : (length - k * step) % length;
      const h = at(phase), before = at(phase - direction * window);
      let stomp = false;
      if (rules.detection) {
        if (landed) { if (before - h > DESCENT) { landed = false; armedAt = phase; } }
        else if (before - h < DESCENT) { stomp = true; landed = true; }
      } else {
        const crossed = before > h && level > h && before > level;
        stomp = crossed && !crossing;
        crossing = crossed;
      }
      let lift = false;
      if (h > before) { if (h - lift0 > rules.hydraulic && !lifted) lift = lifted = true; }
      else lifted = false;
      if (pass && stomp) stomps.push({ at: phase, arm: rules.detection ? armedAt : null });
      if (pass && lift) lifts.push(phase);
    }
  }
  return { stomps, lifts };
}

// A class's walk cycle as its feet see it (null without one): its length,
// type 1 or not, and for each foot when it comes down and lifts, walked
// forward and played backward (a riding animal reversing).
export function gait(v) {
  const def = v.def;
  if (gaits.has(def)) return gaits.get(def);
  let result = null;
  const feet = def?.fx?.feet, length = v.walkLength;
  if (feet?.length && v.pose && length > 0 && v.bones?.length) {
    const rules = { ...RULES };
    for (const key of Object.keys(RULES)) if (Number.isFinite(def.fx.stomp?.[key])) rules[key] = def.fx.stomp[key];
    rules.centred = !feet.some((foot) => foot.bottom);
    const rate = parseFloat(classSetting(def, "AnimationSpeed")) || 1;
    const heights = cycleHeights(v, feet);
    result = { length, detection: Boolean(rules.detection),
      forward: heights.map((h) => footSchedule(h, length, rules, rate, 1)),
      backward: heights.map((h) => footSchedule(h, length, rules, rate, -1)) };
  }
  gaits.set(def, result);
  return result;
}

// How far along the way from `from` by `delta` round a cycle of `length`
// (either way round) `phase` lies: in (0, |delta|], else -1.
function along(phase, from, delta, length) {
  const ahead = (((phase - from) * Math.sign(delta)) % length + length) % length;
  return ahead > 0 && ahead <= Math.abs(delta) ? ahead : -1;
}

export class Footfalls {
  constructor() {
    this.feet = [];
    this.phase = null; // the walk phase last read, null before the first
    this.walking = false;
    this.armed = []; // type 1: each foot has fallen fast since it came down
    this.events = [];
  }

  // Calls onStep(index, point, "down" | "up", turn) for each foot that
  // came down or started to lift this frame: point is where it meets the
  // ground, turn its primitive's turn in the world (null when the class
  // data has none); both are reused between calls.
  update(v, feet, dt, onStep) {
    const g = feet.length ? gait(v) : null;
    if (!g) { this.fall(v, feet, dt, onStep); return; }
    const phase = v.shownWalkPhase ?? v.walkPhase, walking = (v.shownWalkBlend ?? v.walkBlend) > WALKING;
    const from = this.phase, started = walking && !this.walking && from !== null;
    this.phase = phase; this.walking = walking;
    if (from === null || !walking) return;
    if (started) {
      // Standing cleared type 1's state: every foot stomps now, and has
      // landed.
      this.armed = feet.map(() => false);
      if (g.detection) feet.forEach((foot, i) => onStep(i, this.place(v, foot), "down", this.turned));
      return;
    }
    const delta = (((phase - from) % g.length) + g.length * 1.5) % g.length - g.length / 2;
    if (!delta) return;
    const schedule = delta > 0 ? g.forward : g.backward;
    feet.forEach((foot, i) => {
      // The foot's events on the way, in order: arming, stomps (a type 1
      // foot's only once armed since it last came down) and lifts.
      const events = this.events;
      events.length = 0;
      for (const stomp of schedule[i].stomps) {
        const reach = along(stomp.at, from, delta, g.length);
        if (reach >= 0) events.push([reach, "down"]);
        const arming = stomp.arm === null ? -1 : along(stomp.arm, from, delta, g.length);
        if (arming >= 0) events.push([arming, "arm"]);
      }
      for (const lift of schedule[i].lifts) {
        const reach = along(lift, from, delta, g.length);
        if (reach >= 0) events.push([reach, "up"]);
      }
      if (events.length > 1) events.sort((a, b) => a[0] - b[0]);
      for (const [, step] of events) {
        if (step === "arm") { this.armed[i] = true; continue; }
        if (step === "down" && g.detection) { if (!this.armed[i]) continue; this.armed[i] = false; }
        onStep(i, this.place(v, foot), step, this.turned);
      }
    });
  }

  // A foot as drawn: where it meets the ground (returned), and its turn
  // (this.turned, null when the class data has none).
  place(v, foot) {
    const bone = (foot.bone && v.byName?.[foot.bone]) || v.root;
    bone.updateWorldMatrix(true, false);
    _point.fromArray(foot.bottom || foot.at).applyMatrix4(bone.matrixWorld);
    this.turned = foot.turn ? bone.getWorldQuaternion(_turn).multiply(_q.fromArray(foot.turn)) : null;
    return _point;
  }

  // A walker without a walk cycle: each foot as drawn, a foot falling faster
  // than DROP that then stops falling having come down (the port's reading).
  fall(v, feet, dt, onStep) {
    if (dt <= 0) return;
    feet.forEach((foot, i) => {
      const point = this.place(v, foot);
      const f = this.feet[i];
      if (!f) { this.feet[i] = { y: point.y, falling: false, down: false }; return; }
      const fall = (f.y - point.y) / dt;
      f.y = point.y;
      if (fall > DROP) { f.falling = true; f.down = false; }
      else if (f.falling && fall < LANDED) { f.falling = false; f.down = true; onStep(i, point, "down", this.turned); }
      else if (f.down && -fall > DROP) { f.down = false; onStep(i, point, "up", this.turned); }
    });
  }
}
