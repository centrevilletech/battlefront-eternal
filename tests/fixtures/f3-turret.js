import * as THREE from 'three';
import {Turret} from '../../src/props.js';
// Native class policy must survive the desktop bundler's renamed classes.
class GunEmplacement extends Turret {}

// Synthetic construction geometry, shared by native phase and page tests.
export function turretAt({ heading = 0, drumBind = 0, settings = {} } = {}) {
  const rig = { root: new THREE.Group(), byName: {} };
  for (const name of ['drum', 'gun', 'seat', 'hp_fire']) { const o = new THREE.Object3D(); o.name = name; rig.byName[name] = o; }
  rig.root.add(rig.byName.drum); rig.byName.drum.add(rig.byName.gun); rig.byName.gun.add(rig.byName.hp_fire); rig.byName.drum.add(rig.byName.seat);
  rig.byName.drum.position.y = 1.5; rig.byName.drum.quaternion.setFromAxisAngle(THREE.Object3D.DEFAULT_UP, drumBind);
  rig.byName.hp_fire.position.z = 1; rig.byName.seat.position.z = -0.6;
  rig.bones = Object.values(rig.byName);
  rig.model = { box: [[-1, 0, -1], [1, 2, 1]] };
  const matrix = new THREE.Matrix4().makeRotationY(heading);
  rig.root.applyMatrix4(matrix);
  const desc = { base: 'armedbuilding', class: 'test_turret', matrix: matrix.toArray(),
    settings: { TurretNodeName: 'drum', AimerNodeName: 'gun', PilotPosition: 'seat', FirePointName: 'hp_fire', MaxTurnSpeed: 2.85, MaxHealth: 300, AimerYawLimits: '-5 5', ...settings },
    weapon: { class: 'test_turret_gun', base: 'cannon', ShotDelay: 0.4, RoundsPerClip: 0, ReloadTime: 0, ordnance: { base: 'bolt', Velocity: 80, LifeSpan: 2, Damage: 20 } } };
  return new GunEmplacement(desc, { built: rig }, { built: null }, null);
}
