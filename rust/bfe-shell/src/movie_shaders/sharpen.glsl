#version 300 es
precision highp float;

uniform sampler2D source;
uniform float sharpness;
uniform float opacity;
out vec4 color;
void main() {
  ivec2 p = ivec2(gl_FragCoord.xy);
  ivec2 last = textureSize(source, 0) - 1;
  vec3 b = texelFetch(source, clamp(p + ivec2(0, 1), ivec2(0), last), 0).rgb;
  vec3 d = texelFetch(source, clamp(p - ivec2(1, 0), ivec2(0), last), 0).rgb;
  vec3 e = texelFetch(source, p, 0).rgb;
  vec3 f = texelFetch(source, clamp(p + ivec2(1, 0), ivec2(0), last), 0).rgb;
  vec3 h = texelFetch(source, clamp(p - ivec2(0, 1), ivec2(0), last), 0).rgb;
  vec3 lo = min(min(b, d), min(min(e, f), h));
  vec3 hi = max(max(b, d), max(max(e, f), h));
  vec3 hitMin = lo / (4.0 * hi + 1e-5);
  vec3 hitMax = (1.0 - hi) / (4.0 * lo - 4.0 - 1e-5);
  vec3 lobes = max(-hitMin, hitMax);
  float lobe = max(-0.1875, min(max(lobes.r, max(lobes.g, lobes.b)), 0.0)) * sharpness;
  color = vec4(clamp((lobe * (b + d + f + h) + e) / (4.0 * lobe + 1.0), 0.0, 1.0), opacity);
}