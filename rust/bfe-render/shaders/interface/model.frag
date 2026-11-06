
  uniform sampler2D map;
  uniform bool hasMap;
  uniform vec3 color;
  uniform float opacity;
  uniform float lit;
  uniform vec3 toLight;
  uniform float ambient;
  uniform float cutoff;
  uniform float solid;
  uniform vec3 keyTint;
  uniform vec3 rim;
  uniform float rimPower;
  uniform vec4 recolor;
  uniform float cut;
  uniform vec3 cutGlow;
  varying vec2 vUv;
  varying vec3 vNormal;
  varying vec3 vTint;
  varying float vLift;
  void main() {
    if (vLift > cut) discard;
    vec4 texel = hasMap ? texture2D(map, vUv) : vec4(1.0);
    vec3 n = normalize(vNormal) * (gl_FrontFacing ? 1.0 : -1.0);
    vec3 light = vec3(min(ambient + max(dot(n, toLight), 0.0), 1.0)) * keyTint;
    vec4 c = vec4(texel.rgb * color * vTint * mix(vec3(1.0), light, lit), texel.a * opacity);
    c.rgb += rim * pow(1.0 - max(n.z, 0.0), rimPower) * lit;
    if (recolor.a > 0.0) c.rgb = mix(c.rgb, recolor.rgb * dot(c.rgb, vec3(0.299, 0.587, 0.114)) * 2.0, recolor.a);
    c.rgb += cutGlow * (1.0 - smoothstep(0.0, 0.08, cut - vLift));
    if (c.a <= cutoff) discard;
    // An opaque part's texture alpha holds something else (a gloss mask);
    // what it writes is solid.
    gl_FragColor = vec4(c.rgb, mix(c.a, 1.0, solid));
  }
