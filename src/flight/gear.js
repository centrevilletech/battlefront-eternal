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
// (rust/sim flight/gear.rs, before the aimers turn), and each page in its
// native hull phase: an X-wing's lasers ride its S-foils and the MAF's pilot
// cannons its flaps. A landed craft stands on its gear's lowest point
// (gearFoot).
import * as THREE from 'three';
import { Pose } from '../anim.js';

const ROOT = /^dummyroot$/i;
// Each hull's own presentation state, kept off its fields (a prediction
// copy copies those): the life whose records its gear follows, and whether
// it does.
const views = new WeakMap();

// How far below its origin a flyer stands on its gear (negative: above it):
// the lowest point of its model at the takeoff clip's first frame, posed as
// the gear posing poses it and skinned as a page draws it (the batches' vertices,
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
    // Native vehicleConstruction retains exact Float32 bind streams for this
    // actual cold landing query; no render descriptor is reconstructed.
    const source=batch.asset?.authored;if(source?.kind!=='rig')continue;
    const p=source.positions,joints=source.joints,weights=source.weights;
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
// hull taken over from its own replay starts where that left it. The clock
// that moves it is the page's native hull phase's (rust/bfe-presentation
// vehicles.rs shown_gear); this keeps only which life it follows.
export function followFlightState(v, follows) {
  const generation = v.aiTarget?.generation ?? 0;
  const view = views.get(v);
  if (!view) views.set(v, { generation, follows });
  else { view.generation = generation; view.follows = follows; }
}

// Whether a hull's drawn gear follows its records (1), is its own on his
// craft (0) or is its own for want of any record this life (-1).
export function gearFollow(v) {
  const view = views.get(v);
  if (!view || view.generation !== (v.aiTarget?.generation ?? 0)) return -1;
  return view.follows ? 1 : 0;
}
