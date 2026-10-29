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

// A restrained LDR glow, independent of the retail map's soft-focus blur.
// Threshold before averaging, so thin bright bolts survive the downsample.
// No exposure/tone-map change: the original gamma-space palette stays intact.
export class Bloom {
  constructor(source) {
    const target = () => new THREE.WebGLRenderTarget(1, 1, { depthBuffer: false });
    this.target = target();
    this.scratch = target();
    this.extract = fullScreenPass(/* glsl */ `
      uniform sampler2D map;
      uniform vec2 texel;
      varying vec2 vUv;
      vec3 bright(vec2 uv) {
        vec3 c = texture2D(map, uv).rgb;
        float peak = max(c.r, max(c.g, c.b));
        float knee = clamp(peak - 0.65, 0.0, 0.24);
        float contribution = max(peak - 0.77, knee * knee / 0.48);
        return c * contribution / max(peak, 0.0001);
      }
      void main() {
        vec3 c = bright(vUv + texel * vec2(-1.0, -1.0));
        c += bright(vUv + texel * vec2(1.0, -1.0));
        c += bright(vUv + texel * vec2(-1.0, 1.0));
        c += bright(vUv + texel * vec2(1.0, 1.0));
        gl_FragColor = vec4(c * 0.25, 1.0);
      }
    `, { map: { value: source }, texel: { value: new THREE.Vector2() } });
    this.blur = fullScreenPass(/* glsl */ `
      uniform sampler2D map;
      uniform vec2 direction;
      varying vec2 vUv;
      void main() {
        vec3 c = texture2D(map, vUv).rgb * 0.227027;
        c += texture2D(map, vUv + direction * 1.384615).rgb * 0.316216;
        c += texture2D(map, vUv - direction * 1.384615).rgb * 0.316216;
        c += texture2D(map, vUv + direction * 3.230769).rgb * 0.070270;
        c += texture2D(map, vUv - direction * 3.230769).rgb * 0.070270;
        gl_FragColor = vec4(c, 1.0);
      }
    `, { map: { value: null }, direction: { value: new THREE.Vector2() } });
  }

  resize(width, height) {
    this.target.setSize(Math.max(1, Math.ceil(width / 4)), Math.max(1, Math.ceil(height / 4)));
    this.scratch.setSize(this.target.width, this.target.height);
    this.extract.material.uniforms.texel.value.set(1 / width, 1 / height);
  }

  render(renderer, camera) {
    renderer.setRenderTarget(this.target);
    renderer.render(this.extract, camera);
    const u = this.blur.material.uniforms;
    u.map.value = this.target.texture;
    u.direction.value.set(1 / this.target.width, 0);
    renderer.setRenderTarget(this.scratch);
    renderer.render(this.blur, camera);
    u.map.value = this.scratch.texture;
    u.direction.value.set(0, 1 / this.target.height);
    renderer.setRenderTarget(this.target);
    renderer.render(this.blur, camera);
  }
}
