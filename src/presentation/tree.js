import {sceneProperty} from "../rust-render/scene-changes.js";
import { publishSceneWorld } from "../rust-render/scene.js";
// Resource shells are made once. Numeric tree records own their transforms
// afterwards, including skinned meshes parented under a moving drum.
import { poseOf } from './unit-content.js';
import { SceneNode as N } from './layout.js';
function published() {}
const NEGATIVE_ZERO = '~negative zero~';
export const exact = value => JSON.stringify(value, (_, x) => Object.is(x, -0) ? NEGATIVE_ZERO : x).replaceAll(`"${NEGATIVE_ZERO}"`, '-0.0');
// Registration preserves traversal order and records the scene parent.
// Bone indices are explicit, so meshes and static nodes cannot shift them.
export function tree(root, bones = []) {
  const objects = [];
  root.traverse(o => objects.push(o));
  const ids = new Map(objects.map((o, i) => [o, i]));
  return { objects, descriptor: {
    nodes: objects.map(o => ({ parent: ids.get(o.parent) ?? -1, pose: poseOf(o), matrix: Array.from(o.matrix.elements), auto: o.matrixAutoUpdate, skinned: Boolean(o.isSkinnedMesh) })),
    bones: bones.map(b => ids.get(b)), parent: root.parent ? Array.from(root.parent.matrixWorld.elements) : null,
  } };
}
// Called after registration, never while collecting frame inputs. The root
// stops Three's scene walk; mesh updates cannot rewrite the bind inverse.
export function ownTree(row) {
  for (const o of row.objects) {
    sceneProperty(o,"matrixAutoUpdate",sceneProperty(o,"matrixWorldAutoUpdate",false));
    if (o === row.objects[0] || o.isSkinnedMesh) o.updateMatrixWorld = o.updateWorldMatrix = published;
  }
}
// A row changes when either its local or world matrix moved. Stable props
// leave their Three arrays alone, including the attached-mode skin binding.
export function publishTree(row, records) {
  for (let i = 0, at = row.start; i < row.objects.length; i++, at += N.STRIDE) {
    if (!records[at + N.CHANGED]) continue;
    const o = row.objects[i], m = o.matrix.elements, w = o.matrixWorld.elements;
    publishSceneWorld(o, records, at + N.WORLD);
    for (let k = 0; k < 16; k++) { m[k] = records[at + N.MATRIX + k]; w[k] = records[at + N.WORLD + k]; }
    if (o.isSkinnedMesh) {
      const inverse = o.bindMatrixInverse.elements;
      for (let k = 0; k < 16; k++) inverse[k] = records[at + N.BIND_INVERSE + k];
    }
  }
}
