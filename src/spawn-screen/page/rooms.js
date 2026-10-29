import * as THREE from "three";

// What the page spawn screen's stage (stage.js) stands in, by look:
// - GridRoom: the 2004 screen's hologram room, its panels (the shell_cube
//   texture) on every wall, the floor and the ceiling of a room round the
//   camera, scrolling as the original's do, with light running along the
//   lines and pooling on the floor round the soldier. It hides the battle.
// - StageFloor: over the battle, a floor of light under the soldier, a
//   grid fading out from him with rings rippling out across it.

const ROOM = { width: 22, height: 9, depth: 26, back: -13 }; // m, its floor at y 0
const TILE = 1.8; // m: a panel of the texture

const ROOM_VERTEX = /* glsl */ `
  varying vec3 vWorld;
  varying vec3 vNormal;
  void main() {
    vec4 world = modelMatrix * vec4(position, 1.0);
    vWorld = world.xyz;
    vNormal = normalize(mat3(modelMatrix) * normal);
    gl_Position = projectionMatrix * viewMatrix * world;
  }
`;
// Each face takes the texture in its own plane; the panels scroll; bands of
// light sweep along the lines (the texture's bright part); the floor glows
// round the soldier; far is darker.
const ROOM_FRAGMENT = /* glsl */ `
  uniform sampler2D map;
  uniform float time;
  uniform float scroll;
  uniform vec3 tint;
  uniform vec3 pulse;
  uniform float strength;
  varying vec3 vWorld;
  varying vec3 vNormal;
  void main() {
    vec3 n = abs(vNormal);
    // Walls the right way up (the texture's rows run down).
    vec2 uv = n.y > 0.5 ? vWorld.xz : (n.x > 0.5 ? vec2(vWorld.z, -vWorld.y) : vec2(vWorld.x, -vWorld.y));
    uv = uv / ${TILE.toFixed(2)};
    uv.x += time * scroll;
    vec3 t = texture2D(map, uv).rgb;
    float lum = dot(t, vec3(0.299, 0.587, 0.114));
    float line = smoothstep(0.18, 0.42, lum);
    float f = fract(vWorld.x * 0.09 + vWorld.y * 0.06 + vWorld.z * 0.05 - time * 0.13);
    float band = smoothstep(0.0, 0.012, f) * (1.0 - smoothstep(0.012, 0.08, f));
    float g = fract(vWorld.y * 0.085 + time * 0.06);
    float rise = smoothstep(0.0, 0.006, g) * (1.0 - smoothstep(0.006, 0.035, g));
    float pool = exp(-length(vWorld.xz) * 0.32) * step(0.5, n.y);
    float far = 1.0 / (1.0 + length(vWorld - vec3(0.0, 1.0, 0.0)) * 0.05);
    vec3 c = t * tint * (0.8 + 0.9 * pool) * far + pulse * line * (band * 1.5 + rise * 0.9 + pool * 0.6);
    gl_FragColor = vec4(c * strength, 1.0);
  }
`;

export class GridRoom {
  // map: the shell_cube texture (or null, until it loads).
  constructor(map) {
    if (map && map.wrapS !== THREE.RepeatWrapping) {
      map.wrapS = map.wrapT = THREE.RepeatWrapping;
      if (map.image) map.needsUpdate = true;
    }
    this.material = new THREE.ShaderMaterial({
      uniforms: {
        map: { value: map },
        time: { value: 0 },
        scroll: { value: 0.0625 },
        tint: { value: new THREE.Color(1, 1, 1) },
        pulse: { value: new THREE.Color(0.45, 0.75, 1) },
        strength: { value: 0 },
      },
      vertexShader: ROOM_VERTEX,
      fragmentShader: ROOM_FRAGMENT,
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
    renderer.render(this.scene, camera);
  }

  dispose() {
    this.material.dispose();
    this.mesh.geometry.dispose();
  }
}

const FLOOR_VERTEX = /* glsl */ `
  varying vec3 vWorld;
  void main() {
    vec4 world = modelMatrix * vec4(position, 1.0);
    vWorld = world.xyz;
    gl_Position = projectionMatrix * viewMatrix * world;
  }
`;
const FLOOR_FRAGMENT = /* glsl */ `
  uniform vec3 color;
  uniform float time;
  uniform float strength;
  varying vec3 vWorld;
  void main() {
    vec2 p = vWorld.xz;
    float r = length(p);
    vec2 cell = p / 0.6;
    vec2 g = abs(fract(cell - 0.5) - 0.5) / fwidth(cell);
    float grid = 1.0 - min(min(g.x, g.y), 1.0);
    float w = fract(r * 0.55 - time * 0.22);
    float ripple = smoothstep(0.0, 0.03, w) * (1.0 - smoothstep(0.03, 0.2, w));
    float fade = 1.0 - smoothstep(1.0, 7.0, r);
    float a = (grid * 0.2 + ripple * 0.28 + 0.06 * exp(-r * 0.8)) * fade * strength;
    gl_FragColor = vec4(color * a, a);
  }
`;

export class StageFloor {
  constructor() {
    this.material = new THREE.ShaderMaterial({
      uniforms: { color: { value: new THREE.Color(1, 1, 1) }, time: { value: 0 }, strength: { value: 0 } },
      vertexShader: FLOOR_VERTEX,
      fragmentShader: FLOOR_FRAGMENT,
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
