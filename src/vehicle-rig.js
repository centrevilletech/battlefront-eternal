import * as THREE from 'three';
import { partMaterial } from './models.js';
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
  const local = rig.bones.map(b => new THREE.Matrix4().fromArray(b.local));
  const world = new Array(local.length);
  const resolve = i => world[i] || (world[i] = rig.bones[i].parent >= 0
    ? resolve(rig.bones[i].parent).clone().multiply(local[i]) : local[i].clone());
  local.forEach((_, i) => resolve(i));
  world.push(new THREE.Matrix4()); // parts attached directly to the model
  const names = new Map(rig.bones.map((b, i) => [b.name.toLowerCase(), i]));
  // The nearest ancestor among the separate bones of a part's bone, or null.
  const attachOf = (part) => {
    if (!separate.length) return null;
    let i = names.get((part.bone || '').toLowerCase());
    while (i !== undefined && i >= 0) {
      const name = rig.bones[i].name.toLowerCase();
      if (separate.includes(name)) return name;
      i = rig.bones[i].parent;
    }
    return null;
  };
  const groups = new Map();
  for (const part of parts) {
    const attach = attachOf(part);
    const key = JSON.stringify(attach === null && !separate.length ? [part.texture, part.flags, part.scroll] : [part.texture, part.flags, part.scroll, attach]);
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(part);
  }
  const point = new THREE.Vector3(), normal = new THREE.Vector3();
  const batches = [];
  for (const parts of groups.values()) {
    const count = parts.reduce((n, p) => n + p.geometry.attributes.position.count, 0);
    const elements = parts.reduce((n, p) => n + p.geometry.index.count, 0);
    const positions = new Float32Array(count * 3), normals = new Float32Array(count * 3), uvs = new Float32Array(count * 2);
    const joints = new Uint16Array(count * 4), weights = new Float32Array(count * 4), indices = new Uint32Array(elements);
    let vertex = 0, index = 0;
    for (const part of parts) {
      const joint = names.get((part.bone || '').toLowerCase()) ?? rig.bones.length;
      const matrix = world[joint];
      const g = part.geometry, base = vertex;
      for (let i = 0; i < g.attributes.position.count; i++, vertex++) {
        point.fromBufferAttribute(g.attributes.position, i);
        normal.fromBufferAttribute(g.attributes.normal, i);
        if (!part.joints) { point.applyMatrix4(matrix); normal.transformDirection(matrix); }
        point.toArray(positions, vertex * 3); normal.toArray(normals, vertex * 3);
        uvs[vertex * 2] = g.attributes.uv.getX(i); uvs[vertex * 2 + 1] = g.attributes.uv.getY(i);
        joints[vertex * 4] = part.joints?.[i] ?? joint; weights[vertex * 4] = 1;
        // A soft skin blends three bones (the sarlacc tentacle, Jabba).
        if (part.skinJoints) for (let k = 0; k < 3; k++) { joints[vertex * 4 + k] = part.skinJoints[i * 3 + k]; weights[vertex * 4 + k] = part.skinWeights[i * 3 + k]; }
      }
      for (let i = 0; i < g.index.count; i++) indices[index++] = base + g.index.getX(i);
    }
    const geometry = new THREE.BufferGeometry();
    for (const [name, array, size] of [['position', positions, 3], ['normal', normals, 3], ['uv', uvs, 2], ['skinIndex', joints, 4], ['skinWeight', weights, 4]]) {
      geometry.setAttribute(name, new THREE.BufferAttribute(array, size));
    }
    geometry.setIndex(new THREE.BufferAttribute(indices, 1));
    geometry.computeBoundingSphere();
    // Animated barrels and feet may extend beyond the bind-pose sphere.
    geometry.boundingSphere.radius *= 1.35;
    batches.push({ geometry, material: partMaterial(parts[0], texture), casts: !((parts[0].flags ?? 1) & 0x84), attach: attachOf(parts[0]) });
  }
  if (dispose) for (const part of parts) part.geometry.dispose();
  return batches;
}

// options.byName: bone name (lower case) -> bone, for batches with an attach
// bone (vehicleBatches options.separate); options.motion false leaves the
// meshes out of the temporal upscaler's motion pass (placed objects that
// barely move); options.meshes collects the meshes made.
export function attachVehicleBatches(root, bones, batches, caster, { byName = null, motion = true, meshes = null } = {}) {
  const staticBone = new THREE.Bone();
  root.add(staticBone);
  root.updateMatrixWorld(true);
  const skeleton = new THREE.Skeleton([...bones, staticBone]);
  for (const batch of batches) {
    const mesh = new THREE.SkinnedMesh(batch.geometry, batch.material);
    ((batch.attach && byName?.[batch.attach]) || root).add(mesh);
    // Vertices are in the root's bind frame: an identity bind matrix, even
    // for a mesh hung under one of its bones (its attached-mode inverse
    // follows that bone's placement each frame).
    mesh.bind(skeleton, IDENTITY);
    mesh.boundingSphere = batch.geometry.boundingSphere.clone();
    mesh.receiveShadow = true;
    if (batch.casts) caster?.(mesh);
    if (motion) motionVectors.track(mesh, { far: 150 });
    meshes?.push(mesh);
  }
  return skeleton;
}
