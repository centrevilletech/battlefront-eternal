import { shaderSources } from "./render-shaders.js";
import * as THREE from "three";

// One oversized triangle avoids a diagonal seam and redundant fragment quads.
const triangle = new THREE.BufferGeometry();
triangle.setAttribute("position", new THREE.Float32BufferAttribute([-1, -1, 0, 3, -1, 0, -1, 3, 0], 3));
triangle.setAttribute("uv", new THREE.Float32BufferAttribute([0, 0, 2, 0, 0, 2], 2));

export function fullScreenPass(fragmentShader, uniforms, vertexShader = /* glsl */ `
  varying vec2 vUv;
  void main() {
    vUv = uv;
    gl_Position = vec4(position.xy, 0.0, 1.0);
  }
`) {
  const material = new THREE.ShaderMaterial({
    uniforms, vertexShader, fragmentShader,
    depthTest: false, depthWrite: false, blending: THREE.NoBlending,
  });
  const mesh = new THREE.Mesh(triangle, material);
  mesh.frustumCulled = false;
  return mesh;
}

// A post step's values (uniform name -> value) set on its pass's material
// before it is drawn, by whichever renderer draws it (graphics.js postStep,
// rust-render/post.js): one pass drawn several times in a frame with other
// values (bloom's blur) is then an ordinary step each time.
export function applyValues(material, values) {
  const uniforms = material.uniforms;
  for (const name in values) uniforms[name].value = values[name];
}

// A restrained LDR glow, independent of the retail map's soft-focus blur.
// Threshold before averaging, so thin bright bolts survive the downsample.
// No exposure/tone-map change: the original gamma-space palette stays intact.
export class Bloom {
  constructor(source) {
    const target = () => new THREE.WebGLRenderTarget(1, 1, { depthBuffer: false });
    this.target = target();
    this.scratch = target();
    // The blur's directions, across then down, one texel of the target each.
    this.across = new THREE.Vector2();
    this.down = new THREE.Vector2();
    this.extract = ownedFullScreenPass("post-hud/bloom-extract", { map: { value: source }, texel: { value: new THREE.Vector2() } });
    this.blur = ownedFullScreenPass("post-hud/bloom-blur", { map: { value: null }, direction: { value: new THREE.Vector2() } });
  }

  resize(width, height) {
    this.target.setSize(Math.max(1, Math.ceil(width / 4)), Math.max(1, Math.ceil(height / 4)));
    this.scratch.setSize(this.target.width, this.target.height);
    this.extract.material.uniforms.texel.value.set(1 / width, 1 / height);
  }

  // Its passes as the frame's post steps (graphics.js postSteps): the bright
  // parts into the quarter-size target, blurred across into the scratch
  // target, then down back into the target, the blur's one pass drawn twice
  // with other values.
  steps(camera) {
    this.across.set(1 / this.target.width, 0);
    this.down.set(0, 1 / this.target.height);
    return [
      { pass: this.extract, target: this.target, camera },
      { pass: this.blur, target: this.scratch, camera, values: { map: this.target.texture, direction: this.across } },
      { pass: this.blur, target: this.target, camera, values: { map: this.scratch.texture, direction: this.down } },
    ];
  }
}

// Rust owns the source; three uses the same text when it draws a fallback.
export function ownedFullScreenPass(key, uniforms) {
  const source = shaderSources[key];
  const pass = fullScreenPass(source.fragment, uniforms, source.vertex);
  pass.material.userData.bfeProgram = key;
  return pass;
}
