uniform float size;
uniform float scale;
void main() {
  vec3 transformed = vec3(position);
  vec4 mvPosition = vec4(transformed, 1.0);
  mvPosition = modelViewMatrix * mvPosition;
  gl_Position = projectionMatrix * mvPosition;
  gl_PointSize = size;
  bool isPerspective = (projectionMatrix[2][3] == -1.0);
  if (isPerspective) gl_PointSize *= (scale / -mvPosition.z);
}
