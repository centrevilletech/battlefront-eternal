float planeShadow(highp sampler2DShadow depthMap, vec2 size, float bias, vec3 coord, vec2 gradient) {
if (coord.z < 0.0 || coord.z > 1.0 || any(lessThan(coord.xy, vec2(0.0))) || any(greaterThan(coord.xy, vec2(1.0)))) return 1.0;
vec2 pixel = coord.xy * size - 0.5;
vec2 base = floor(pixel);
vec2 f = fract(pixel);
// A fetch's four texels share its reference depth, so the plane is off
// by up to a texel's slope at them. Where that is more than 6 cm (the
// depth range is 3000 m), as on a wall nearly edge-on to the sun whose
// own back face is the nearest caster, compare every texel on the plane.
vec2 slope = abs(gradient) / size;
if (max(slope.x, slope.y) > 2e-5) {
vec4 wx = vec4(1.0 - f.x, 1.0, 1.0, f.x) / 3.0;
vec4 wy = vec4(1.0 - f.y, 1.0, 1.0, f.y) / 3.0;
float result = 0.0;
for (int y = 0; y < 4; y++) {
for (int x = 0; x < 4; x++) {
vec2 uv = (base + vec2(float(x), float(y)) - 0.5) / size;
float planeOffset = clamp(dot(gradient, uv - coord.xy), -0.002, 0.002);
result += wx[x] * wy[y] * texture(depthMap, vec3(uv, coord.z + planeOffset + bias));
}
}
return result;
}
vec2 wa = 2.0 - f;
vec2 wb = 1.0 + f;
vec2 a = (base - 0.5 + 1.0 / wa) / size;
vec2 b = (base + 1.5 + f / wb) / size;
float reference = coord.z + bias;
float result = wa.x * wa.y * texture(depthMap, vec3(a, reference + clamp(dot(gradient, a - coord.xy), -0.002, 0.002)));
result += wb.x * wa.y * texture(depthMap, vec3(b.x, a.y, reference + clamp(dot(gradient, vec2(b.x, a.y) - coord.xy), -0.002, 0.002)));
result += wa.x * wb.y * texture(depthMap, vec3(a.x, b.y, reference + clamp(dot(gradient, vec2(a.x, b.y) - coord.xy), -0.002, 0.002)));
result += wb.x * wb.y * texture(depthMap, vec3(b, reference + clamp(dot(gradient, b - coord.xy), -0.002, 0.002)));
return result / 9.0;
}
#endif
// The sun's shadow from the cascade covering this depth: 1 lit, 0 dark.
float sunShadow() {
float shadow = 1.0;
