
  varying vec3 vView;
  void main() {
    vec4 view = modelViewMatrix * vec4(position, 1.0);
    vView = view.xyz;
    gl_Position = projectionMatrix * view;
  }
