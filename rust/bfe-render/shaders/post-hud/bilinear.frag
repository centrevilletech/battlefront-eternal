
  uniform sampler2D map;
  varying vec2 vUv;
  void main() { gl_FragColor = vec4(texture2D(map, vUv).rgb, 1.0); }
