#ifdef USE_MAP
uniform mat3 mapTransform;
varying vec2 vMapUv;
#endif
void main() {
#ifdef USE_MAP
  vMapUv = (mapTransform * vec3(uv, 1)).xy;
#endif
  vec3 transformed = vec3(position);
  vec4 mvPosition = vec4(transformed, 1.0);
  mvPosition = modelViewMatrix * mvPosition;
  gl_Position = projectionMatrix * mvPosition;
}
