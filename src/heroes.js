import * as THREE from "three";
import {actorAsset,glowNode,glowLook} from './actor-resources.js';

// Page weapon state for native melee snapshots and drawn blade selection.
// The native combat owner alone advances swings and acquires their targets.
export function prepareMelee(arsenal) {
  for (const weapon of arsenal.weapons) {
    if (weapon.def.base !== "melee") continue;
    weapon.bottomless = true;
    weapon.clip = 1;
    weapon.reserve = Infinity;
    weapon.melee = new MeleeSwing(weapon);
  }
  return arsenal;
}

const meleeSwings=weapon=>weapon.def.swings?.length?weapon.def.swings.slice(0,3):[{duration:1,hit:.5,sound:null}];
export class MeleeSwing {
  constructor(weapon) {
    this.weapon = weapon;
    this.time = -1;
    this.swing = null;
    this.index = 0;
    this.active = false; // a swing under way (`swing` set): the store's
  }

  captureState(){return {time:this.time,index:this.index,active:Boolean(this.swing)};}
  restoreState(state){this.time=state.time;this.index=state.index;this.swing=state.active?meleeSwings(this.weapon)[state.index]:null;this.active=Boolean(this.swing);}
}

// The blade lives under the existing held hilt, so visibility, pooling and
// hand animation use the same lifecycle. Constant native axis and widths;
// additive core blend is provisional (the native core blend is untraced).
export function attachBlade(group, base, def, texture) {
  if (!base || !def.Texture) return null;
  const axis = new THREE.Vector3(0, .8, .5);
  const length = axis.length();
  axis.normalize();
  const core = texture(def.Texture);
  if (!core) return null;
  const blade = new THREE.Group();
  blade.position.fromArray(base);
  group.add(blade);
  const cameraLocal = new THREE.Vector3();
  const right = new THREE.Vector3();
  const normal = new THREE.Vector3();
  const basis = new THREE.Matrix4();
  for (const [map, width, opacity] of [[core, .08, 1], [texture("lightsabreglow"), .16, 100 / 255]]) {
    if (!map) continue;
    const mesh = glowNode(actorAsset({kind:'plane',width,height:length}),glowLook(map,0xffffff,opacity));
    mesh.position.copy(axis).multiplyScalar(length / 2);
    mesh.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), axis);
    mesh.onBeforeRender = (_renderer, _scene, camera) => {
      camera.getWorldPosition(cameraLocal);
      blade.worldToLocal(cameraLocal);
      right.crossVectors(axis, cameraLocal).normalize();
      if (right.lengthSq() < 1e-9) return;
      normal.crossVectors(right, axis).normalize();
      mesh.quaternion.setFromRotationMatrix(basis.makeBasis(right, axis, normal));
      mesh.updateMatrixWorld(true);
    };
    blade.add(mesh);
  }
  return blade;
}
