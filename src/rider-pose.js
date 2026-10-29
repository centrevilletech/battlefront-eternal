import * as THREE from "three";
import { Pose } from "./anim.js";

// A soldier's body as an exposed seat poses it, for the simulation: what a
// round meets of a rider, and where his body falls from when he dies there.
//
// The 2004 game puts a shown rider's collision in its own list, which
// every round's ray reaches (docs/formats.md "Riders"). The port's riders are
// drawn from their seat's clip (src/vehicles.js poseRider), so their hit
// shapes come from the same clip at the same point in it, carried down the
// rider's own skeleton here without touching the bones that are drawn. The
// animator does the same sampling for the picture (SoldierAnimator.gunner):
// joint rotations from the clip, every bone's bind length but the root's
// (DummyRoot stays at its bind place on a seat), bone_root where the clip
// puts it. Nothing here depends on what was drawn, so a world worker and a
// dedicated server, which pose no bodies, meet the rider where a client
// shows him.

// The head's middle in the head bone's frame (x runs up the neck): the
// average of the head vertices of the troopers' bodies.
const HEAD_MIDDLE = new THREE.Vector3(0.05, 0.04, 0);
const HEAD_RADIUS = 0.17; // m, by max(CollisionRootScale, 1), as on foot (src/units.js)
const TORSO_RADIUS = 0.25; // m, pelvis to neck
const THIGH_RADIUS = 0.13; // m, hip to knee
const SHIN_RADIUS = 0.11; // m, knee to ankle
// The arms, measured on the clone's body: the upper arm's sleeve reaches
// about 0.09 m from its bone, the forearm's and the glove's 0.07; the
// glove's middle lies 0.1 m down the hand bone from the wrist (x runs down
// every limb bone), its fingertips 0.2.
const UPPER_ARM_RADIUS = 0.09; // m, shoulder to elbow
const FOREARM_RADIUS = 0.07; // m, elbow to the middle of the hand
const HAND_MIDDLE = new THREE.Vector3(0.1, -0.02, 0);
// The joints a rider's points hang from; the arms' last, which a skeleton
// may lack (its rider is then struck without them).
const NAMES = ["DummyRoot", "bone_root", "bone_pelvis", "bone_neck", "bone_head",
  "bone_l_thigh", "bone_l_calf", "bone_l_foot", "bone_r_thigh", "bone_r_calf", "bone_r_foot"];
const ARMS = ["bone_l_upperarm", "bone_l_forearm", "bone_l_hand", "bone_r_upperarm", "bone_r_forearm", "bone_r_hand"];

// Per skeleton: each bone's parent and the bones the rider's points hang
// from, in an order that puts every parent before its children.
const skeletons = new WeakMap();
function skeletonOf(bones) {
  let skeleton = skeletons.get(bones);
  if (skeleton) return skeleton;
  const index = new Map(bones.map((bone, i) => [bone, i]));
  const parent = bones.map((bone) => index.get(bone.parent) ?? -1);
  const byName = Object.fromEntries(bones.map((bone, i) => [bone.name, i]));
  const order = [];
  const seen = new Set();
  const visit = (i) => {
    if (i < 0 || seen.has(i)) return;
    visit(parent[i]);
    seen.add(i);
    order.push(i);
  };
  const arms = ARMS.every((name) => byName[name] !== undefined);
  for (const name of arms ? [...NAMES, ...ARMS] : NAMES) if (byName[name] !== undefined) visit(byName[name]);
  skeleton = { parent, byName, order, root: byName.DummyRoot ?? 0, arms };
  if (NAMES.slice(2).some((name) => byName[name] === undefined)) skeleton.incomplete = true;
  skeletons.set(bones, skeleton);
  return skeleton;
}

const poses = new WeakMap();
const local = { q: [], p: [] };
const world = { q: [], p: [] };
const scratchQ = new THREE.Quaternion();
const binds = new WeakMap();

// The bind lengths the animator restores after every pose (src/animator.js
// BIND_LENGTH), by bone index, from its own record of them.
function bindsOf(animator, bones) {
  let bind = binds.get(animator);
  if (bind) return bind;
  bind = new Map();
  for (const [bone, position] of animator.bindLengths || []) bind.set(bones.indexOf(bone), position);
  binds.set(animator, bind);
  return bind;
}

// The rider's points in his body's frame (the frame the actor's root is
// drawn in: feet at the origin, y up), for a seat sample { clip, pitch,
// turn, time } (src/vehicles.js riderSample): pelvis, neck, head middle,
// both hips, knees and ankles, and both shoulders, elbows and hands' middles
// (out.arms says whether the skeleton has arms). Written into out; null for
// a skeleton the points can't be found on.
export function riderPoints(animator, sample, out) {
  const bones = animator?.bones;
  const clip = sample?.clip;
  if (!bones || !clip) return null;
  const skeleton = skeletonOf(bones);
  if (skeleton.incomplete) return null;
  let pose = poses.get(bones);
  if (!pose) poses.set(bones, (pose = new Pose(bones.length, skeleton.root)));
  // As SoldierAnimator.update samples a seat's clip.
  pose.clear();
  if (sample.time !== null && sample.time !== undefined) pose.add(clip, sample.time, 1);
  else {
    const pitch = THREE.MathUtils.clamp(sample.pitch || 0, -1, 1), turn = THREE.MathUtils.clamp(sample.turn || 0, -1, 1);
    pose.add(clip, (4 + turn) / clip.fps, 1 - Math.abs(pitch), false);
    pose.add(clip, ((pitch > 0 ? 7 : 1) + turn) / clip.fps, Math.abs(pitch), false);
  }
  const bind = bindsOf(animator, bones);
  for (const i of skeleton.order) {
    const w = pose.w[i];
    const q = (local.q[i] ||= new THREE.Quaternion());
    const p = (local.p[i] ||= new THREE.Vector3());
    if (w > 1e-6) q.set(pose.q[i * 4], pose.q[i * 4 + 1], pose.q[i * 4 + 2], pose.q[i * 4 + 3]).normalize();
    else q.copy(bones[i].quaternion);
    if (i === skeleton.root && animator.rootBind) p.copy(animator.rootBind);
    else if (bind.has(i)) p.copy(bind.get(i));
    else if (w > 1e-6) p.set(pose.t[i * 3] / w, pose.t[i * 3 + 1] / w, pose.t[i * 3 + 2] / w);
    else p.copy(bones[i].position);
    const up = skeleton.parent[i];
    const wq = (world.q[i] ||= new THREE.Quaternion());
    const wp = (world.p[i] ||= new THREE.Vector3());
    if (up < 0) { wq.copy(q); wp.copy(p); }
    else { wq.multiplyQuaternions(world.q[up], q); wp.copy(p).applyQuaternion(world.q[up]).add(world.p[up]); }
  }
  const at = (name, target) => target.copy(world.p[skeleton.byName[name]]);
  at("bone_pelvis", out.pelvis);
  at("bone_neck", out.neck);
  const head = skeleton.byName.bone_head;
  out.head.copy(HEAD_MIDDLE).applyQuaternion(scratchQ.copy(world.q[head])).add(world.p[head]);
  at("bone_l_thigh", out.hips[0]); at("bone_r_thigh", out.hips[1]);
  at("bone_l_calf", out.knees[0]); at("bone_r_calf", out.knees[1]);
  at("bone_l_foot", out.ankles[0]); at("bone_r_foot", out.ankles[1]);
  out.arms = Boolean(skeleton.arms && out.shoulders);
  if (out.arms) {
    ["l", "r"].forEach((side, i) => {
      at(`bone_${side}_upperarm`, out.shoulders[i]);
      at(`bone_${side}_forearm`, out.elbows[i]);
      const hand = skeleton.byName[`bone_${side}_hand`];
      out.hands[i].copy(HAND_MIDDLE).applyQuaternion(world.q[hand]).add(world.p[hand]);
    });
  }
  // His root bone's place and turn, where `out` asks for them (a creature
  // hangs him by it: src/props.js AnimatedProp.heldFrame).
  if (out.root) at("bone_root", out.root);
  if (out.rootTurn) out.rootTurn.copy(world.q[skeleton.byName.bone_root]);
  return out;
}

export function makeRiderPoints() {
  const v = () => new THREE.Vector3();
  return { pelvis: v(), neck: v(), head: v(), hips: [v(), v()], knees: [v(), v()], ankles: [v(), v()],
    shoulders: [v(), v()], elbows: [v(), v()], hands: [v(), v()], arms: false };
}

// Hit shapes (src/registry.js segmentShape) from a rider's points carried
// into the world by `frame` (a Matrix4): the head, a sphere whose hits do
// three times the damage, as on foot; the torso from the pelvis up to the
// neck (its rounded top ending there, below the head, as the on-foot
// body's cylinder ends below his), each leg's thigh and shin, and where
// the skeleton has them each arm's upper arm and forearm to the middle of
// the hand, as capsules. A gunner's arms reach out to the grips well clear
// of his torso's capsule, where rounds were seen to pass through them.
// scale: the class's CollisionRootScale. shapes: RIDER_SHAPES reusable
// entries. Returns the count: 10, or 6 without arms.
export const RIDER_SHAPES = 10;
const a = new THREE.Vector3(), b = new THREE.Vector3(), up = new THREE.Vector3();
export function riderHitShapes(points, frame, scale, shapes) {
  const S = Math.max(scale ?? 1, 1);
  sphere(shapes[0], "head", a.copy(points.head).applyMatrix4(frame), HEAD_RADIUS * S);
  a.copy(points.pelvis).applyMatrix4(frame);
  b.copy(points.neck).applyMatrix4(frame);
  const spine = up.subVectors(b, a).length();
  if (spine > TORSO_RADIUS) b.addScaledVector(up.divideScalar(spine), -TORSO_RADIUS);
  capsule(shapes[1], "body", a, b, TORSO_RADIUS);
  for (let side = 0; side < 2; side++) {
    capsule(shapes[2 + side * 2], "body", a.copy(points.hips[side]).applyMatrix4(frame), b.copy(points.knees[side]).applyMatrix4(frame), THIGH_RADIUS);
    capsule(shapes[3 + side * 2], "body", a.copy(points.knees[side]).applyMatrix4(frame), b.copy(points.ankles[side]).applyMatrix4(frame), SHIN_RADIUS);
  }
  if (!points.arms) return 6;
  for (let side = 0; side < 2; side++) {
    capsule(shapes[6 + side * 2], "body", a.copy(points.shoulders[side]).applyMatrix4(frame), b.copy(points.elbows[side]).applyMatrix4(frame), UPPER_ARM_RADIUS);
    capsule(shapes[7 + side * 2], "body", a.copy(points.elbows[side]).applyMatrix4(frame), b.copy(points.hands[side]).applyMatrix4(frame), FOREARM_RADIUS);
  }
  return 10;
}

function sphere(shape, part, center, radius) {
  shape.type = "sphere";
  shape.part = part;
  shape.center.copy(center);
  shape.radius = radius;
  shape.half = 0;
  shape.yaw = 0;
}

function capsule(shape, part, from, to, radius) {
  shape.type = "capsule";
  shape.part = part;
  shape.center.addVectors(from, to).multiplyScalar(0.5);
  shape.half = shape.halfSize.subVectors(to, from).multiplyScalar(0.5);
  shape.radius = radius;
  shape.yaw = 0;
}

// Where a rider killed on his seat starts to fall from (his feet, the
// body's origin): under his pelvis, his soles where the lower of his drawn
// feet was, so the body leaves the seat where it was seen. In the body's
// frame.
const ANKLE = 0.13; // m: a standing soldier's ankles above his soles
export function riderDropPoint(points, out) {
  return out.set(points.pelvis.x, Math.min(points.ankles[0].y, points.ankles[1].y) - ANKLE, points.pelvis.z);
}
