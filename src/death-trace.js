import {sceneProperty} from "./rust-render/scene-changes.js";
import { EFFECT } from './rust-render/effect-layout.js';
import * as THREE from "three";

// The killing shot's trace for the Enhanced death camera (src/death-shot.js),
// as a broadcast replay draws a ball's flight: a glowing line from the killer
// to the body. Its head runs from him to the body with the line glowing
// behind it, then it fades. It is drawn over the scene, as a telestrator's
// line is, additive: red for an enemy's shot, green for a teammate's. Only
// the page builds it.
import {EffectPhase} from './effects-gpu.js';
import {TraceFrame as F} from './presentation/visual-effects-layout.js';
const BANKS=[['input',Float64Array],['positions',Float32Array]];

export class DeathTrace {
  constructor(scene) {
    this.phase=new EffectPhase('PageDeathTrace',BANKS);
    this.positions = new Float32Array(12);
    // DeathTrace publishes only its moving positions and trace facts. The
    // native descriptor owns line coordinates, indices and additive state.
    this.nativeKind=EFFECT.DEATH_TRACE.ID;this.values=new Float64Array(EFFECT.DEATH_TRACE.LENGTH);
    this.values.set([1,.34,.18],EFFECT.DEATH_TRACE.COLOR);this.values[EFFECT.DEATH_TRACE.SPAN]=1;
    this.vertex=4;this.index=6;this.arrays={position:this.positions};this.visibility='anchor';
    this.mesh=new THREE.Object3D();this.mesh.userData.effectMesh=this;
    // Found by this name by a renderer that draws it itself (src/rust-render/effects.js).
    this.mesh.name = "death-trace";
    sceneProperty(this.mesh,"frustumCulled",false);
    sceneProperty(this.mesh,"renderOrder",990);
    sceneProperty(this.mesh,"visible",false);
    scene.add(this.mesh);
  }

  // from: the killer; to: the body; head: how far along its head has run
  // (0 to 1); opacity; time: s since it started (its shimmer); seen from
  // camera, as drawn this frame.
  show(from, to, friendly, head, opacity, time, camera) {
    this.phase.refresh();const x=this.phase.input;
    from.toArray(x,F.START);to.toArray(x,F.END);camera.position.toArray(x,F.CAMERA);
    x[F.FRIENDLY]=friendly?1:0;x[F.HEAD]=head;x[F.OPACITY]=opacity;x[F.TIME]=time;
    this.phase.frame();this.positions.set(this.phase.positions);
    const v=this.values;for(let i=0;i<3;i++)v[EFFECT.DEATH_TRACE.COLOR+i]=x[F.COLOR+i];
    v[EFFECT.DEATH_TRACE.HEAD]=x[F.HEAD];v[EFFECT.DEATH_TRACE.OPACITY]=x[F.OPACITY];v[EFFECT.DEATH_TRACE.TRACE_TIME]=x[F.TIME];v[EFFECT.DEATH_TRACE.SPAN]=x[F.SPAN];
    sceneProperty(this.mesh,"visible",Boolean(x[F.VISIBLE]));
  }

  hide() {
    sceneProperty(this.mesh,"visible",false);
  }

  dispose() {
    this.phase.dispose();this.mesh.removeFromParent();
  }
}
