
  attribute vec2 maskUv;
  attribute vec4 tint;
  varying vec2 vUv;
  varying vec2 vMaskUv;
  varying vec4 vTint;
  void main() {
    vUv = uv;
    vMaskUv = maskUv;
    vTint = tint;
    gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
  }
