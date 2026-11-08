import {sceneProperty} from "../rust-render/scene-changes.js";
import {TreePublication} from "../rust-render/pose-streams.js";
import {PoseTreeKind} from "../rust-render/pose-layout.js";
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
    o.updateMatrixWorld=o.updateWorldMatrix=published;
  }
  row.publication=new TreePublication(row.poseSource,row.objects,PoseTreeKind.TREE);
}
// A row changes when either its local or world matrix moved. Stable props
// leave their Three arrays alone, including the attached-mode skin binding.
export function publishTree(row,records){row.publication.publish(records,row.start);}
