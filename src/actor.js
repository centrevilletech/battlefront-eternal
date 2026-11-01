import * as THREE from "three";
import { zeroMaterial } from "./materials.js";
import { motionVectors } from "./motion-vectors.js";

const IDENTITY = new THREE.Matrix4();

// Geometry is immutable and shared by actors built from the same source.
// Skeletons and lighting uniforms remain per actor.
const geometryCache = new WeakMap();
const NO_EULER = () => {};

// A soldier's body: a hard-skinned mesh on its skeleton, one material per
// texture its parts use (the jet trooper's pack has its own). Vertices are
// packed as position, normal, uv, and bone index (9 floats). The held weapon
// hangs off the hand's hp_weapons (`hand`).
//
// overrides: textures to draw in place of others (a class's OverrideTexture
// re-skins its body: the assault droid blue, the assassin red).
export function buildActor(actor, actorBuf, actorIndexBuf, textureFor, graphics, overrides = {}) {
  const actorVerts = new Float32Array(actorBuf);
  const actorIndices = new Uint32Array(actorIndexBuf);

  const root = new THREE.Group();
  const byName = {};
  const bones = actor.bones.map((desc, index) => {
    const bone = new THREE.Bone();
    bone.name = desc.name;
    bone.userData.index = index;
    new THREE.Matrix4().fromArray(desc.local).decompose(bone.position, bone.quaternion, bone.scale);
    // Animation sets every bone's quaternion every frame; three would also
    // recompute its Euler angles each time (a rotation matrix and atan2s per
    // bone), and nothing reads a bone's Euler rotation.
    bone.quaternion._onChange(NO_EULER);
    byName[desc.name] = bone;
    return bone;
  });
  for (let i = 0; i < actor.bones.length; i++) {
    const parent = actor.bones[i].parent;
    if (parent >= 0) bones[parent].add(bones[i]);
  }

  let buffers = geometryCache.get(actor);
  if (!buffers) geometryCache.set(actor, buffers = new WeakMap());
  let indices = buffers.get(actorBuf);
  if (!indices) buffers.set(actorBuf, indices = new WeakMap());
  let cached = indices.get(actorIndexBuf);
  if (!cached) indices.set(actorIndexBuf, cached = new Map());
  const key = JSON.stringify(overrides);
  const saved = cached.get(key);
  let geometry = saved?.geometry;
  const materialTextures = saved?.materialTextures || [];
  if (!saved) {
    let count = 0;
    for (const part of actor.parts) count += part.vertexCount;
    const positions = new Float32Array(count * 3);
    const normals = new Float32Array(count * 3);
    const uvs = new Float32Array(count * 2);
    const skinIndex = new Uint16Array(count * 4);
    const skinWeight = new Float32Array(count * 4);
    const index = [];

    const slots = new Map(); // texture name -> material index
    const groups = [];
    let cursor = 0;
    for (const part of actor.parts) {
      const texture = overrides[part.texture] || part.texture;
      if (!slots.has(texture)) {
        slots.set(texture, materialTextures.length);
        // Soldiers are dynamic objects: the sky-to-ground ambient, the sun as
        // the light region allows, and the terrain's light map under their feet.
        materialTextures.push([texture, part.texture]);
      }
      groups.push([index.length, part.indexCount, slots.get(texture)]);
      const source = part.vertexOffset / 4;
      for (let i = 0; i < part.vertexCount; i++) {
        const at = source + i * 9;
        positions.set(actorVerts.subarray(at, at + 3), cursor * 3);
        normals.set(actorVerts.subarray(at + 3, at + 6), cursor * 3);
        uvs.set(actorVerts.subarray(at + 6, at + 8), cursor * 2);
        skinIndex[cursor * 4] = actorVerts[at + 8];
        skinWeight[cursor * 4] = 1;
        cursor += 1;
      }
      // Indices in the file already point into the packed actor buffer.
      const indexAt = part.indexOffset / 4;
      for (let k = 0; k < part.indexCount; k++) index.push(actorIndices[indexAt + k]);
    }
    geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.BufferAttribute(positions, 3));
    geometry.setAttribute("normal", new THREE.BufferAttribute(normals, 3));
    geometry.setAttribute("uv", new THREE.BufferAttribute(uvs, 2));
    geometry.setAttribute("skinIndex", new THREE.Uint16BufferAttribute(skinIndex, 4));
    geometry.setAttribute("skinWeight", new THREE.Float32BufferAttribute(skinWeight, 4));
    geometry.setIndex(index);
    for (const [start, n, material] of groups) geometry.addGroup(start, n, material);

    cached.set(key, { geometry, materialTextures });
  }
  const materials = materialTextures.map(([texture, fallback]) =>
    zeroMaterial("dynamic", { map: textureFor(texture) || textureFor(fallback) }));

  const body = new THREE.SkinnedMesh(geometry, materials);
  // A fixed sphere around the soldier (feet at the origin) that any pose stays
  // inside, prone and dying included. Skinned bounds computed from one pose
  // go stale as the clips play; with culling off, every soldier on the map
  // was drawn in the view, the reflection and all four shadow cascades.
  body.boundingSphere = new THREE.Sphere(new THREE.Vector3(0, 0.9, 0), 2.2);
  // ...which also bounds which point lights can reach it (materials.js).
  body.userData.poseSphere = true;
  body.receiveShadow = true;
  const skeleton = new THREE.Skeleton(bones);
  root.add(body);
  body.add(bones[0]);
  body.bind(skeleton);
  // Every vertex follows one bone at full weight (above): with an identity
  // bind matrix the shader reads that bone alone (materials.js RIGID_SKIN).
  if (body.bindMatrix.equals(IDENTITY)) for (const material of materials) material.defines.RIGID_SKIN = "";

  // The body's shadow in one draw per cascade: its material groups were a
  // draw each, though a depth map needs no materials. This proxy shares the
  // skeleton and buffers and draws only in the shadow pass (the hidden layer
  // the shadow camera adds); the body itself no longer casts. units.js hides
  // it for distant soldiers.
  const shadowGeometry = new THREE.BufferGeometry();
  shadowGeometry.index = geometry.index;
  shadowGeometry.attributes = geometry.attributes;
  const shadow = new THREE.SkinnedMesh(shadowGeometry, materials[0]);
  shadow.name = "body-shadow";
  shadow.boundingSphere = body.boundingSphere;
  shadow.layers.set(graphics.hiddenLayer ?? 1);
  root.add(shadow);
  shadow.bind(skeleton, body.bindMatrix);
  graphics.caster(shadow);
  // Its screen motion for the temporal upscaler.
  motionVectors.track(body);

  return { root, bones, byName, skeleton, body, shadow, hand: byName[actor.attach], materials };
}
