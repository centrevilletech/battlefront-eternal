#ifdef HEATED
uniform float heat;
uniform vec4 heatTear; // where it tore away (its own frame), and how far the heat spreads
uniform float heatBase; // heat everywhere, besides the tear's
uniform float heatScale; // the hot patches' size: 1 for pieces, less for bigger patches
uniform float soot;
uniform float dissolve;
varying vec3 vHeatLocal;
float hHash(vec3 p) {
p = fract(p * 0.3183099 + 0.1);
p *= 17.0;
return fract(p.x * p.y * p.z * (p.x + p.y + p.z));
}
float hNoise(vec3 x) {
vec3 i = floor(x), f = fract(x);
f = f * f * (3.0 - 2.0 * f);
return mix(mix(mix(hHash(i), hHash(i + vec3(1, 0, 0)), f.x), mix(hHash(i + vec3(0, 1, 0)), hHash(i + vec3(1, 1, 0)), f.x), f.y),
mix(mix(hHash(i + vec3(0, 0, 1)), hHash(i + vec3(1, 0, 1)), f.x), mix(hHash(i + vec3(0, 1, 1)), hHash(i + vec3(1, 1, 1)), f.x), f.y), f.z);
}
vec3 heated(vec3 color) {
float n = hNoise(vHeatLocal * 13.0 * heatScale) * 0.62 + hNoise(vHeatLocal * 34.0 * heatScale + 5.0) * 0.38;
if (n < dissolve) discard;
float tear = max(exp(-distance(vHeatLocal, heatTear.xyz) / max(heatTear.w, 0.01)), heatBase);
float scorch = clamp(tear * 1.25 + (n - 0.45) * 0.9, 0.0, 1.0);
color *= 1.0 - soot * (0.18 + 0.55 * scorch);
// Ragged hot patches, densest where it tore; white at the hottest.
float glow = smoothstep(0.52, 0.92, tear * 0.8 + n * 0.55) * heat;
vec3 hot = mix(vec3(0.7, 0.07, 0.01), vec3(1.9, 0.7, 0.16), smoothstep(0.2, 0.7, heat));
hot = mix(hot, vec3(2.4, 1.85, 1.15), smoothstep(0.75, 1.0, heat) * smoothstep(0.55, 0.9, glow));
return color + hot * glow;
}
#endif
