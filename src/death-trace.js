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

const VERTEX = /* glsl */ `
attribute vec2 line;
varying vec2 vLine;
void main() {
  vLine = line;
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}`;

// line.x: 0 at the killer, 1 at the body; line.y: -1 to 1 across.
const FRAGMENT = /* glsl */ `
uniform vec3 color;
uniform float head;
uniform float opacity;
uniform float span;
uniform float time;
varying vec2 vLine;
void main() {
  float along = vLine.x;
  if (along > head) discard;
  float across = vLine.y;
  float core = exp(-across * across * 40.0);
  float glow = exp(-across * across * 4.0);
  float behind = (head - along) * span;
  float spark = exp(-behind * behind * 0.6);
  float tail = 0.35 + 0.65 * exp(-behind / max(span * 0.5, 6.0));
  float pulse = 0.9 + 0.1 * sin(along * span * 0.5 - time * 14.0);
  float ends = smoothstep(0.0, 0.02, along);
  float light = (core * 1.4 + glow * 0.35) * (tail * pulse + spark * 1.8) * ends * opacity;
  gl_FragColor = vec4(color * light, 1.0);
}`;

export class DeathTrace {
  constructor(scene) {
    this.positions = new Float32Array(12);
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.BufferAttribute(this.positions, 3).setUsage(THREE.DynamicDrawUsage));
    geometry.setAttribute("line", new THREE.BufferAttribute(new Float32Array([0, -1, 0, 1, 1, -1, 1, 1]), 2));
    geometry.setIndex([0, 2, 1, 1, 2, 3]);
    this.material = new THREE.ShaderMaterial({
      uniforms: { color: { value: ENEMY.clone() }, head: { value: 0 }, opacity: { value: 0 }, span: { value: 1 }, time: { value: 0 } },
      vertexShader: VERTEX,
      fragmentShader: FRAGMENT,
      transparent: true,
      depthWrite: false,
      depthTest: false,
      side: THREE.DoubleSide,
      blending: THREE.AdditiveBlending,
    });
    this.mesh = new THREE.Mesh(geometry, this.material);
    this.mesh.frustumCulled = false;
    this.mesh.renderOrder = 990;
    this.mesh.visible = false;
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
    const u = this.material.uniforms;
    u.color.value.copy(friendly ? FRIEND : ENEMY);
    u.head.value = head;
    u.opacity.value = opacity;
    u.time.value = time;
    const dir = this.dir.subVectors(to, from);
    u.span.value = Math.max(dir.length(), 1e-3);
    dir.normalize();
    // Each end turned to face the eye.
    const p = this.positions;
    for (const [end, at] of [[from, 0], [to, 6]]) {
      const toEye = this.toEye.subVectors(camera.position, end);
      const width = Math.max(WIDTH, toEye.length() * FAR_WIDTH) / 2;
      const side = this.side.crossVectors(dir, toEye);
      if (side.lengthSq() < 1e-10) side.set(0, 1, 0);
      side.normalize().multiplyScalar(width);
      p[at] = end.x - side.x; p[at + 1] = end.y - side.y; p[at + 2] = end.z - side.z;
      p[at + 3] = end.x + side.x; p[at + 4] = end.y + side.y; p[at + 5] = end.z + side.z;
    }
    this.mesh.geometry.attributes.position.needsUpdate = true;
    this.mesh.visible = true;
  }

  hide() {
    this.mesh.visible = false;
  }

  dispose() {
    this.mesh.removeFromParent();
    this.mesh.geometry.dispose();
    this.material.dispose();
  }
}
