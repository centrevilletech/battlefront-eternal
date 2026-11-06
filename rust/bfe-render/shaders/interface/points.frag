uniform vec3 diffuse;
uniform float opacity;
uniform sampler2D map;
uniform mat3 uvTransform;
void main() {
  vec4 diffuseColor = vec4(diffuse, opacity);
  vec2 uv = (uvTransform * vec3(gl_PointCoord.x, 1.0 - gl_PointCoord.y, 1)).xy;
  diffuseColor *= texture2D(map, uv);
  vec3 outgoingLight = diffuseColor.rgb;
  gl_FragColor = vec4(outgoingLight, diffuseColor.a);
}
