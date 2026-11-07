import {sceneComponent,sceneProperty,sceneTransform} from "../../rust-render/scene-changes.js";
import {interfaceNode} from '../../interface/resources.js';
import {repeatImage} from '../../interface/images.js';
import {LOOK,paintArray} from '../../rust-render/interface-material.js';
import { surfaceMaterial as interfaceMaterial } from "../../rust-render/interface-material.js";
import { renderInterface } from "../../rust-render/draw-guard.js";
import * as THREE from "three";

// What the page spawn screen's stage (stage.js) stands in, by look:
// - GridRoom: the 2004 screen's hologram room, its panels (the shell_cube
//   texture) on every wall, the floor and the ceiling of a room round the
//   camera, scrolling as the original's do, with light running along the
//   lines and pooling on the floor round the soldier. It hides the battle.
// - StageFloor: over the battle, a floor of light under the soldier, a
//   grid fading out from him with rings rippling out across it.

const ROOM = { width: 22, height: 9, depth: 26, back: -13 }; // m, its floor at y 0

// Each face takes the texture in its own plane; the panels scroll; bands of
// light sweep along the lines (the texture's bright part); the floor glows
// round the soldier; far is darker.

export class GridRoom {
  // map: the shell_cube texture (or null, until it loads).
  constructor(map) {
    map=repeatImage(map);
    sceneProperty(this,"material",interfaceMaterial("room", {
      uniforms: {
        map: { value: map },
        time: { value: 0 },
        scroll: { value: 0.0625 },
        tint: { value: new THREE.Color(1, 1, 1) },
        pulse: { value: new THREE.Color(0.45, 0.75, 1) },
        strength: { value: 0 },
      },
      side: THREE.BackSide,
      depthTest: false,
      depthWrite: false,
    }));
    this.mesh = interfaceNode({kind:"box",width:ROOM.width,height:ROOM.height,depth:ROOM.depth},this.material);
    sceneTransform(this.mesh.position.set(0, ROOM.height / 2, ROOM.back + ROOM.depth / 2));
    sceneProperty(this.mesh,"frustumCulled",false);
    this.scene = new THREE.Scene();
    this.scene.add(this.mesh);
  }

  colors({ tint, pulse }) {
    if (tint) paintArray(this.material,LOOK.TINT,tint);
    if (pulse) paintArray(this.material,LOOK.PULSE,pulse);
  }

  update(time, strength) {
    this.material.values[LOOK.TIME] = time;
    this.material.values[LOOK.STRENGTH] = strength;
  }

  // Under everything (it is the room the camera stands in).
  render(renderer, camera) {
    if (!this.material.map) return;
    renderInterface(renderer, this.scene, camera, "spawn-room");
  }

  dispose() {
    this.mesh.removeFromParent();
  }
}


export class StageFloor {
  constructor() {
    sceneProperty(this,"material",interfaceMaterial("floor", {
      uniforms: { color: { value: new THREE.Color(1, 1, 1) }, time: { value: 0 }, strength: { value: 0 } },
      transparent: true,
      depthWrite: false,
      blending: THREE.AdditiveBlending,
      side: THREE.DoubleSide,
    }));
    this.mesh = interfaceNode({kind:"circle",radius:7.5,segments:96,transforms:[["rotateX",-Math.PI/2]]},this.material);
    sceneComponent(this.mesh.position,"y","=",-0.005);
    sceneProperty(this.mesh,"renderOrder",-3);
  }

  colors({ color }) {
    if (color) paintArray(this.material,LOOK.COLOR,color);
  }

  update(time, strength) {
    this.material.values[LOOK.TIME] = time;
    this.material.values[LOOK.STRENGTH] = strength;
  }

  dispose() {
    this.mesh.removeFromParent();
  }
}
