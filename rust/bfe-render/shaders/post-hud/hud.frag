
  uniform sampler2D map;
  uniform sampler2D mask;
  uniform bool masked;
  varying vec2 vUv;
  varying vec2 vMaskUv;
  varying vec4 vTint;
  void main() {
    vec4 color = texture2D(map, vUv) * vTint;
    if (masked) color.a *= texture2D(mask, vMaskUv).a;
    gl_FragColor = color;
  }
