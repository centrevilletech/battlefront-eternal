uniform vec3 diffuse;
uniform float opacity;
#ifdef USE_MAP
uniform sampler2D map;
varying vec2 vMapUv;
#endif
void main() {
  vec4 diffuseColor = vec4(diffuse, opacity);
#ifdef USE_MAP
  vec4 sampledDiffuseColor = texture2D(map, vMapUv);
  diffuseColor *= sampledDiffuseColor;
#endif
  vec3 indirectDiffuse = vec3(1.0);
  indirectDiffuse *= diffuseColor.rgb;
  vec3 outgoingLight = indirectDiffuse;
  gl_FragColor = vec4(outgoingLight, diffuseColor.a);
}
