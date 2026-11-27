layout(location = 0) out highp vec4 pc_fragColor;
uniform sampler2D map;
in vec2 vUv;
void main() {
  vec4 sky = texture(map, vUv);
  #ifdef ALPHA_TEST
    if (sky.a < 0.5) discard;
  #endif
  #ifdef BLENDED
    pc_fragColor = sky;
  #else
    pc_fragColor = vec4(sky.rgb, 1.0);
  #endif
}
