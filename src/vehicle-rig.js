import {sceneTransform} from "./rust-render/scene-changes.js";
import {actorAsset,actorNode} from './actor-resources.js';
import * as THREE from 'three';
import { partMaterial } from './models.js';
import { vehicleConstruction } from './anim.js';
import { motionVectors } from './motion-vectors.js';

const IDENTITY = new THREE.Matrix4();

// Pack rigid parts and per-vertex creature skins into one draw per material.
// A soft-skinned part (skinJoints, skinWeights: three a vertex) blends its
// vertices' three bones, as the original's soft skins do.
// Wheels, barrels and hulls can share a texture without a draw per bone. The
// buffers/materials are immutable and shared by every instance of the class.
// options.separate: bone names (lower case) whose parts go into batches of
// their own, parented under that bone (attachVehicleBatches), so code that
// hides a turret's drum and gun by walking those bones still finds their
// meshes. options.dispose false keeps the parts' geometry (a shared pack).
export function vehicleBatches(rig, parts, texture, { separate = [], dispose = true } = {}) {
  // Input parts are resource records, not live nodes. Native packing owns
  // grouping, bind-frame transforms, influences, indices and bounds.
  const records = vehicleConstruction({ rig, separate, parts: parts.map(part => {
    const first=part.vertexOffset/4,data=Array.from(part.asset.vertices.subarray(first,first+part.vertexCount*8));
    const base=part.vertexOffset/32,index=Array.from(part.asset.indices.subarray(part.indexOffset/4,part.indexOffset/4+part.indexCount),v=>v-base);
    return { data, index, texture: part.texture, flags: part.flags, scroll: part.scroll, bone: part.bone,
      joints: part.joints, skinJoints: part.skinJoints, skinWeights: part.skinWeights };
  }) }, true);
  const batches = records.map(record => {
    const asset=actorAsset({kind:'rig',...record});
    return {asset,material:partMaterial(parts[record.part],texture),casts:record.casts,attach:record.attach};
  });
  return batches;
}

// options.byName: bone name (lower case) -> bone, for batches with an attach
// bone (vehicleBatches options.separate); options.motion false leaves the
// meshes out of the temporal upscaler's motion pass (placed objects that
// barely move); options.meshes collects the meshes made.
export function attachVehicleBatches(root, bones, batches, caster, { byName = null, motion = true, meshes = null, staticBone = new THREE.Bone(), inverses = null } = {}) {
  root.add(staticBone);
  if (!inverses) root.updateMatrixWorld(true);
  const skeleton = new THREE.Skeleton([...bones, staticBone], inverses || []);
  for (const batch of batches) {
    const mesh = actorNode(batch.asset,batch.material,true);
    ((batch.attach && byName?.[batch.attach]) || root).add(mesh);
    // Vertices are in the root's bind frame: an identity bind matrix, even
    // for a mesh hung under one of its bones (its attached-mode inverse
    // follows that bone's placement each frame).
    mesh.bind(skeleton, IDENTITY);
    mesh.receiveShadow = true;
    if (batch.casts) caster?.(mesh);
    if (motion) motionVectors.track(mesh, { far: 150 });
    meshes?.push(mesh);
  }
  return skeleton;
}

// Vehicle's native rig records are shared with the world builder. The host
// only instantiates Three resource shells and connects numeric node handles.
export function vehicleShell(vehicle, template, turnRate) {
  const animated = new Set();
  for (const clip of template.clips?.values() || []) for (const track of clip.tracks) if (track >= 0) animated.add(track);
  const records = vehicleConstruction({ class: vehicle.def, rig: template.rig, muzzleTips: Array.from(template.muzzleTips || []),
    animated: Array.from(animated), hasClips: Boolean(template.clips?.size), turnRate, wheelRadii: Array.from(template.wheelRadii || []),
    bounds: [template.shape.localBounds.min.x,template.shape.localBounds.min.y,template.shape.localBounds.min.z,
      template.shape.localBounds.max.x,template.shape.localBounds.max.y,template.shape.localBounds.max.z] });
  const count = template.rig.bones.length;
  const nodes = records.nodes.map((n, i) => {
    const node = i === 0 ? new THREE.Group() : i <= count || i === records.staticBone ? new THREE.Bone() : new THREE.Object3D();
    node.name = n.name;
    sceneTransform(node.position.fromArray(n.pose)); sceneTransform(node.quaternion.fromArray(n.pose, 3)); sceneTransform(node.scale.fromArray(n.pose, 7));
    return node;
  });
  for (let i = 1; i < nodes.length; i++) nodes[records.nodes[i].parent].add(nodes[i]);
  const root = nodes[0], bones = nodes.slice(1, count + 1);
  root.name = vehicle.name;
  const byName = Object.fromEntries(bones.map(b => [b.name.toLowerCase(), b]));
  const bind = new Map(bones.map(b => [b, b.quaternion.clone()]));
  return { root, bones, byName, bind, bindQuaternions: bones.map(b => bind.get(b)), bindPositions: bones.map(b => b.position.clone()),
    nodes, records, staticBone: nodes[records.staticBone], inverses: records.inverses.map(matrix => new THREE.Matrix4().fromArray(matrix)) };
}
