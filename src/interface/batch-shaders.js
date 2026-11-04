// Generated from Rust HUD sources for front-end interfaces and holomaps.
// Battle HUD programs are compiled only by Rust.
export const VERTEX = "\n  attribute vec2 maskUv;\n  attribute vec4 tint;\n  varying vec2 vUv;\n  varying vec2 vMaskUv;\n  varying vec4 vTint;\n  void main() {\n    vUv = uv;\n    vMaskUv = maskUv;\n    vTint = tint;\n    gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);\n  }\n";
export const FRAGMENT = "\n  uniform sampler2D map;\n  uniform sampler2D mask;\n  uniform bool masked;\n  varying vec2 vUv;\n  varying vec2 vMaskUv;\n  varying vec4 vTint;\n  void main() {\n    vec4 color = texture2D(map, vUv) * vTint;\n    if (masked) color.a *= texture2D(mask, vMaskUv).a;\n    gl_FragColor = color;\n  }\n";
