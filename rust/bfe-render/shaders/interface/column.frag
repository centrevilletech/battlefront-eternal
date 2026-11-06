// SpawnStage (column): Bright at its foot, gone toward its top, brighter at its edges, scan
// lines rising through it.

  uniform vec3 color;
  uniform float time;
  uniform float strength;
  varying vec2 vUv;
  varying vec3 vNormal;
  varying vec3 vView;
  void main() {
    float edge = 1.0 - abs(dot(normalize(vNormal), vView));
    float fade = pow(1.0 - vUv.y, 1.6);
    float lines = 0.75 + 0.25 * sin((vUv.y * 46.0 - time * 3.0) * 6.2832);
    float a = fade * (0.08 + 0.5 * pow(edge, 2.2)) * lines * strength;
    gl_FragColor = vec4(color * a, a);
  }
