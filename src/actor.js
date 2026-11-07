import {sceneLayer,sceneProperty,sceneTransform} from "./rust-render/scene-changes.js";
import {actorAsset,actorDescription,actorNode} from './actor-resources.js';
import * as THREE from "three";
import { actorMaterial } from "./materials.js";
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
    sceneTransform(bone.quaternion._onChange(NO_EULER));
    byName[desc.name] = bone;
    return bone;
  });
  for (let i = 0; i < actor.bones.length; i++) {
    const parent = actor.bones[i].parent;
    if (parent >= 0) bones[parent].add(bones[i]);
  }

  let buffers=geometryCache.get(actor);if(!buffers)geometryCache.set(actor,buffers=new WeakMap());
  let indices=buffers.get(actorBuf);if(!indices)buffers.set(actorBuf,indices=new WeakMap());
  let variants=indices.get(actorIndexBuf);if(!variants)indices.set(actorIndexBuf,variants=new Map());
  const key=JSON.stringify(overrides);let asset=variants.get(key);
  if(!asset){asset=actorAsset({kind:'body',parts:actor.parts,overrides},actorVerts,actorIndices);variants.set(key,asset);}
  const facts=actorDescription(asset),materials=facts.textures.map(([texture,fallback])=>actorMaterial('dynamic',{map:textureFor(texture)||textureFor(fallback)}));
  const body=actorNode(asset,materials,true);
  // A fixed sphere around the soldier (feet at the origin) that any pose stays
  // inside, prone and dying included. Skinned bounds computed from one pose
  // go stale as the clips play; with culling off, every soldier on the map
  // was drawn in the view, the reflection and all four shadow cascades.
  sceneProperty(body,"boundingSphere",new THREE.Sphere(new THREE.Vector3(0, 0.9, 0), 2.2));
  // ...which also bounds which point lights can reach it (materials.js).
  body.userData.poseSphere = true;
  body.receiveShadow = true;
  const skeleton = new THREE.Skeleton(bones);
  root.add(body);
  body.add(bones[0]);
  body.bind(skeleton);
  // Every vertex follows one bone at full weight (above): with an identity
  // bind matrix the shader reads that bone alone (materials.js RIGID_SKIN).
  if (body.bindMatrix.equals(IDENTITY)) for (const material of materials) material.authored.rigidSkin = true;

  // The body's shadow in one draw per cascade: its material groups were a
  // draw each, though a depth map needs no materials. This proxy shares the
  // skeleton and buffers and draws only in the shadow pass (the hidden layer
  // the shadow camera adds); the body itself no longer casts. units.js hides
  // it for distant soldiers.
  const shadow=actorNode(asset,materials[0],true);shadow.userData.actorWhole=true;
  shadow.name = "body-shadow";
  sceneProperty(shadow,"boundingSphere",body.boundingSphere);
  sceneLayer(shadow.layers,"set",graphics.hiddenLayer ?? 1);
  root.add(shadow);
  shadow.bind(skeleton, body.bindMatrix);
  graphics.caster(shadow);
  // Its screen motion for the temporal upscaler.
  motionVectors.track(body);
  // What the Rust renderer (rust-render/instances.js) draws of him: the
  // body, and the guns combat.js hangs in his hand.
  root.userData.parts = { body, hand: byName[actor.attach] };

  return { root, bones, byName, skeleton, body, shadow, hand: byName[actor.attach], materials };
}
