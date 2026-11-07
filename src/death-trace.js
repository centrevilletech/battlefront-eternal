import {sceneProperty} from "./rust-render/scene-changes.js";
import { EFFECT } from './rust-render/effect-layout.js';
import * as THREE from "three";

// The killing shot's trace for the Enhanced death camera (src/death-shot.js),
// as a broadcast replay draws a ball's flight: a glowing line from the killer
// to the body. Its head runs from him to the body with the line glowing
// behind it, then it fades. It is drawn over the scene, as a telestrator's
// line is, additive: red for an enemy's shot, green for a teammate's. Only
// the page builds it.
const ENEMY = new THREE.Color(1, 0.34, 0.18);
const FRIEND = new THREE.Color(0.3, 1, 0.45);
const WIDTH = 0.34; // m across the glow, near
const FAR_WIDTH = 0.006; // and at least this much of the distance from the eye



// line.x: 0 at the killer, 1 at the body; line.y: -1 to 1 across.


export class DeathTrace {
  constructor(scene) {
    this.positions = new Float32Array(12);
    // DeathTrace publishes only its moving positions and trace facts. The
    // native descriptor owns line coordinates, indices and additive state.
    this.nativeKind=EFFECT.DEATH_TRACE.ID;this.values=new Float64Array(EFFECT.DEATH_TRACE.LENGTH);
    ENEMY.toArray(this.values,EFFECT.DEATH_TRACE.COLOR);this.values[EFFECT.DEATH_TRACE.SPAN]=1;
    this.vertex=4;this.index=6;this.arrays={position:this.positions};this.visibility='anchor';
    this.mesh=new THREE.Object3D();this.mesh.userData.effectMesh=this;
    // Found by this name by a renderer that draws it itself (src/rust-render/effects.js).
    this.mesh.name = "death-trace";
    sceneProperty(this.mesh,"frustumCulled",false);
    sceneProperty(this.mesh,"renderOrder",990);
    sceneProperty(this.mesh,"visible",false);
    scene.add(this.mesh);
    this.dir = new THREE.Vector3();
    this.side = new THREE.Vector3();
    this.toEye = new THREE.Vector3();
  }

  // from: the killer; to: the body; head: how far along its head has run
  // (0 to 1); opacity; time: s since it started (its shimmer); seen from
  // camera, as drawn this frame.
  show(from, to, friendly, head, opacity, time, camera) {
    if (opacity <= 0 || head <= 0) {
      this.hide();
      return;
    }
    const v=this.values;(friendly?FRIEND:ENEMY).toArray(v,EFFECT.DEATH_TRACE.COLOR);
    v[EFFECT.DEATH_TRACE.HEAD]=head;v[EFFECT.DEATH_TRACE.OPACITY]=opacity;v[EFFECT.DEATH_TRACE.TRACE_TIME]=time;
    const dir = this.dir.subVectors(to, from);
    v[EFFECT.DEATH_TRACE.SPAN] = Math.max(dir.length(), 1e-3);
    dir.normalize();
    // Each end turned to face the eye.
    const p = this.positions;
    for(let i=0;i<2;i++){
      const end=i===0?from:to,at=i*6;
      const toEye = this.toEye.subVectors(camera.position, end);
      const width = Math.max(WIDTH, toEye.length() * FAR_WIDTH) / 2;
      const side = this.side.crossVectors(dir, toEye);
      if (side.lengthSq() < 1e-10) side.set(0, 1, 0);
      side.normalize().multiplyScalar(width);
      p[at] = end.x - side.x; p[at + 1] = end.y - side.y; p[at + 2] = end.z - side.z;
      p[at + 3] = end.x + side.x; p[at + 4] = end.y + side.y; p[at + 5] = end.z + side.z;
    }
    sceneProperty(this.mesh,"visible",true);
  }

  hide() {
    sceneProperty(this.mesh,"visible",false);
  }

  dispose() {
    this.mesh.removeFromParent();
  }
}
