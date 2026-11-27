#version 300 es
precision highp float;
uniform sampler2D map;
in vec2 vUv;
in vec3 vDir;
layout(location = 0) out highp vec4 pc_fragColor;

vec3 hash33(vec3 p) {
  p = fract(p * vec3(0.1031, 0.1030, 0.0973));
  p += dot(p, p.yxz + 33.33);
  return fract((p.xxy + p.yxx) * p.zyx);
}

vec3 starLayer(vec3 dir, float scale, float threshold, float intensity) {
  vec3 a = abs(dir);
  vec2 uv;
  float face;
  if (a.x >= a.y && a.x >= a.z) {
    uv = dir.yz / a.x;
    face = dir.x > 0.0 ? 0.0 : 1.0;
  } else if (a.y >= a.x && a.y >= a.z) {
    uv = dir.xz / a.y;
    face = dir.y > 0.0 ? 2.0 : 3.0;
  } else {
    uv = dir.xy / a.z;
    face = dir.z > 0.0 ? 4.0 : 5.0;
  }
  vec2 grid = uv * scale;
  float px = max(length(fwidth(grid)), 0.02);
  vec2 cell = floor(grid);
  vec2 f = fract(grid);
  vec3 h = hash33(vec3(cell, face + scale * 0.07));
  if (h.z < threshold) return vec3(0.0);
  vec2 center = 0.2 + 0.6 * h.xy;
  float dist = length(f - center);
  float radius = mix(0.05, 0.13, fract(h.z * 37.1)) + px * 0.55;
  float core = 1.0 - smoothstep(0.0, radius, dist);
  float bright = pow((h.z - threshold) / (1.0 - threshold), 1.6) * intensity;
  // Natural star color temperatures: cool blue-white, pure white, soft warm gold.
  vec3 tint = mix(vec3(0.75, 0.86, 1.0), vec3(1.0, 0.93, 0.80), fract(h.x * 19.3));
  return tint * (core * core * bright);
}

void main() {
  vec3 base = texture(map, vUv).rgb;
  vec3 dir = normalize(vDir);
  vec3 stars = starLayer(dir, 95.0, 0.986, 1.25)
             + starLayer(dir, 185.0, 0.978, 0.82)
             + starLayer(dir, 320.0, 0.968, 0.48);
  pc_fragColor = vec4(base + stars, 1.0);
}
