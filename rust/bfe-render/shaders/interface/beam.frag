// HoloMap (beam): Bright at the projector, gone toward the disc, brighter at its edges,
// streaks running up it.

  uniform vec3 color;
  uniform float time;
  uniform float strength;
  varying vec2 vUv;
  varying vec3 vNormal;
  varying vec3 vView;
  void main() {
    float edge = 1.0 - abs(dot(normalize(vNormal), vView));
    float fade = pow(1.0 - vUv.y, 1.3) * smoothstep(0.0, 0.08, vUv.y) + 0.12 * vUv.y;
    float streaks = 0.7 + 0.3 * sin(vUv.x * 6.2832 * 9.0 + time * 1.7) * sin(vUv.x * 6.2832 * 4.0 - time * 1.1);
    float lines = 0.8 + 0.2 * sin((vUv.y * 30.0 - time * 2.4) * 6.2832);
    float a = fade * (0.05 + 0.32 * pow(edge, 2.0)) * streaks * lines * strength;
    gl_FragColor = vec4(color * a, a);
  }
