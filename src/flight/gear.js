// A flyer's landing gear, gear doors and S-foils, drawn from its animation
// bank's "takeoff" clip (tools/zero_vehicle.py; docs/vehicles.md "Landing
// gear"): the clip's first frame is the craft landed (gear down, doors open,
// an X-wing's foils closed), its last the craft in flight (gear stowed, doors
// shut, foils open). The craft's flight state picks the frame: gear 1 is the
// first, 0 the last, as the takeoff and landing move it (src/flight/model.js:
// it stows over TakeoffTime once the craft is TakeoffHeight up, and comes
// down over the landing). A craft without the clip (the TIEs, the droid and
// Geonosian fighters, the gunship, the snowspeeder, the cloud car) keeps the
// pose it was modelled in. The world poses it too, at the craft's own gear
// (poseGearAt in Vehicle.aimTick, before the aimers turn, as the render pass
// every host runs after it does): an X-wing's lasers ride its S-foils and the
// MAF's pilot cannons its flaps. A landed craft stands on its gear's lowest
// point (gearFoot).
import * as THREE from 'three';
import { Pose } from '../anim.js';
import { heightAbove } from './model.js';

const ROOT = /^dummyroot$/i;
// Each hull's own presentation state, kept off its fields (a prediction
// copy copies those): whether its gear follows the flight state alone, and
// how far down that has it.
const views = new WeakMap();
// One pose buffer and root bone per class (its clips), shared by its hulls.
const poses = new WeakMap();
const clamp01 = (x) => Math.min(1, Math.max(0, x));
// The gear a flight state rests at: down on the ground and through the lift,
// stowed in flight (a landing brings it down from there).
const resting = (state) => (state === 'Flying' || state === 'Landing' ? 0 : 1);

// How far below its origin a flyer stands on its gear (negative: above it):
// the lowest point of its model at the takeoff clip's first frame, posed as
// poseGearAt poses it and skinned as a page draws it (the batches' vertices,
// in the model's bind frame, each on its joints). The class's own data in
// plain arithmetic (no trigonometry), so the world, every page and every
// replay get the same number on every platform. Null without the clip or
// any vertices.
export function gearFoot(rig, batches, clip) {
  if (!clip?.frames || !rig?.bones?.length) return null;
  const count = rig.bones.length;
  const bones = rig.bones.map((bone) => {
    const node = { position: new THREE.Vector3(), quaternion: new THREE.Quaternion(), scale: new THREE.Vector3() };
    new THREE.Matrix4().fromArray(bone.local).decompose(node.position, node.quaternion, node.scale);
    return node;
  });
  const bind = bones.map((b) => new THREE.Matrix4().compose(b.position, b.quaternion, b.scale));
  const pose = new Pose(count, -1);
  pose.add(clip, 0, 1, false);
  const posed = bones.map((b) => ({ position: b.position.clone(), quaternion: b.quaternion.clone() }));
  pose.apply(posed);
  const root = rig.bones.findIndex((bone) => ROOT.test(bone.name));
  if (root >= 0) { posed[root].position.copy(bones[root].position); posed[root].quaternion.copy(bones[root].quaternion); }
  const local = posed.map((b, i) => new THREE.Matrix4().compose(b.position, b.quaternion, bones[i].scale));
  const chain = (locals) => {
    const world = new Array(count);
    const resolve = (i) => world[i] || (world[i] = rig.bones[i].parent >= 0 ? resolve(rig.bones[i].parent).clone().multiply(locals[i]) : locals[i].clone());
    for (let i = 0; i < count; i++) resolve(i);
    return world;
  };
  const bindWorld = chain(bind), posedWorld = chain(local);
  // Each joint's skinning matrix (posed over bind); the static bone after
  // the rig's, for parts on the model itself, stays put.
  const skin = posedWorld.map((m, i) => m.clone().multiply(bindWorld[i].clone().invert()).elements);
  skin.push(new THREE.Matrix4().elements);
  let low = Infinity;
  for (const batch of batches || []) {
    const attributes = batch.geometry?.attributes;
    if (!attributes?.position || !attributes.skinIndex || !attributes.skinWeight) continue;
    const p = attributes.position.array, joints = attributes.skinIndex.array, weights = attributes.skinWeight.array;
    for (let v = 0, n = p.length / 3; v < n; v++) {
      const x = p[v * 3], y = p[v * 3 + 1], z = p[v * 3 + 2];
      let height = 0;
      for (let k = 0; k < 4; k++) {
        const w = weights[v * 4 + k];
        if (!w) continue;
        const e = skin[joints[v * 4 + k]] || skin[count];
        height += w * (e[1] * x + e[5] * y + e[9] * z + e[13]);
      }
      if (height < low) low = height;
    }
  }
  return Number.isFinite(low) ? -low : null;
}

// A hull whose flight this host neither steps nor is corrected on (a page
// drawing another player's or a bot's craft from its records, src/runtime/
// network-entity-game.js) has no gear of its own: `follows` makes its gear
// follow its replicated flight state as the takeoff and landing move it. A
// hull taken over from its own replay starts where that left it.
export function followFlightState(v, follows) {
  const generation = v.aiTarget?.generation ?? 0;
  let view = views.get(v);
  if (!view || view.generation !== generation) {
    views.set(v, { generation, follows, state: v.flightState, time: 0, value: follows ? resting(v.flightState) : clamp01(v.gear ?? 1) });
    return;
  }
  if (follows && !view.follows) Object.assign(view, { state: v.flightState, time: 0, value: clamp01(v.gear ?? view.value) });
  view.follows = follows;
}

// How far down the gear is drawn: 1 down, 0 stowed. The hull's own gear
// where its flight is stepped (the world, a pilot's replay) or corrected;
// else its flight state's, advanced by the frame's dt: held down through the
// lift, stowed over TakeoffTime once the craft is TakeoffHeight above where
// it rests, down over LandingTime from the start of a landing (the world's
// rules).
export function shownGear(v, dt = 0) {
  const view = views.get(v);
  if (!view?.follows || view.generation !== (v.aiTarget?.generation ?? 0)) return clamp01(v.gear ?? 1);
  const c = v.flightConfig;
  if (!c) return view.value;
  if (view.state !== v.flightState) { view.state = v.flightState; view.time = 0; }
  else view.time += dt;
  const stow = dt / Math.max(c.takeoffTime, 1e-3), lower = dt / Math.max(c.landingTime, 1e-3);
  switch (v.flightState) {
    case 'Landed': view.value = Math.min(1, view.value + lower); break;
    case 'Landing': view.value = Math.min(1, view.value + lower); break;
    case 'TakingOff': if (dt > 0 && heightAbove(v) > c.takeoffHeight + c.landedHeight) view.value = Math.max(0, view.value - stow); break;
    case 'Flying': view.value = Math.max(0, view.value - stow); break;
    default: break;
  }
  return view.value;
}

// The takeoff clip at the drawn gear (dt: the frame's, for a hull following
// its flight state).
export function poseFlightGear(v, dt = 0) {
  if (!v.flying || !v.clips?.get('takeoff')) return;
  poseGearAt(v, shownGear(v, dt));
}

// The takeoff clip at a gear (1 down, 0 stowed): the world's at the craft's
// own (Vehicle.aimTick, before its aimers and fire points are read), a
// page's at the drawn one. Its DummyRoot track, a constant lift that seats
// the original's model on its origin, is left out: the port seats a hull by
// its collision and gear (gearFoot).
export function poseGearAt(v, gear) {
  const clip = v.flying && v.clips?.get('takeoff');
  if (!clip) return;
  let entry = poses.get(v.clips);
  if (!entry) poses.set(v.clips, entry = { pose: new Pose(v.bones.length, -1), root: v.bones.findIndex((bone) => ROOT.test(bone.name)) });
  const { pose, root } = entry;
  pose.clear();
  pose.add(clip, (1 - clamp01(gear)) * clip.duration, 1, false);
  pose.apply(v.bones);
  if (root >= 0) {
    v.bones[root].position.copy(v.bindPositions[root]);
    v.bones[root].quaternion.copy(v.bindQuaternions[root]);
  }
}
