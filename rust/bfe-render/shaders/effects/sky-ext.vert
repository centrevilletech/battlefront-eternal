uniform mat4 projectionMatrix;
uniform mat4 modelViewMatrix;
#ifdef SCROLL
uniform float time;
uniform vec2 scroll;
#endif
in vec3 position;
in vec2 uv;
out vec2 vUv;
void main() {
  vUv = uv;
  #ifdef SCROLL
    vUv -= scroll * time;
  #endif
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}
