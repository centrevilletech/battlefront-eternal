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
    if (map && map.wrapS !== THREE.RepeatWrapping) {
      map.wrapS = map.wrapT = THREE.RepeatWrapping;
      if (map.image) map.needsUpdate = true;
    }
    this.material = interfaceMaterial("room", {
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
    });
    this.mesh = new THREE.Mesh(new THREE.BoxGeometry(ROOM.width, ROOM.height, ROOM.depth), this.material);
    this.mesh.position.set(0, ROOM.height / 2, ROOM.back + ROOM.depth / 2);
    this.mesh.frustumCulled = false;
    this.scene = new THREE.Scene();
    this.scene.add(this.mesh);
  }

  colors({ tint, pulse }) {
    if (tint) this.material.uniforms.tint.value.setRGB(...tint);
    if (pulse) this.material.uniforms.pulse.value.setRGB(...pulse);
  }

  update(time, strength) {
    this.material.uniforms.time.value = time;
    this.material.uniforms.strength.value = strength;
  }

  // Under everything (it is the room the camera stands in).
  render(renderer, camera) {
    if (!this.material.uniforms.map.value) return;
    renderInterface(renderer, this.scene, camera, "spawn-room");
  }

  dispose() {
    this.material.dispose();
    this.mesh.geometry.dispose();
  }
}


export class StageFloor {
  constructor() {
    this.material = interfaceMaterial("floor", {
      uniforms: { color: { value: new THREE.Color(1, 1, 1) }, time: { value: 0 }, strength: { value: 0 } },
      transparent: true,
      depthWrite: false,
      blending: THREE.AdditiveBlending,
      side: THREE.DoubleSide,
    });
    this.mesh = new THREE.Mesh(new THREE.CircleGeometry(7.5, 96).rotateX(-Math.PI / 2), this.material);
    this.mesh.position.y = -0.005;
    this.mesh.renderOrder = -3;
  }

  colors({ color }) {
    if (color) this.material.uniforms.color.value.setRGB(...color);
  }

  update(time, strength) {
    this.material.uniforms.time.value = time;
    this.material.uniforms.strength.value = strength;
  }

  dispose() {
    this.material.dispose();
    this.mesh.geometry.dispose();
  }
}
