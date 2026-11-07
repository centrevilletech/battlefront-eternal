weight = 1.0 - smoothstep(mix(interval.x, interval.y, 0.9), interval.y, linearDepth);
#endif
#if 2 > 0
previous = cascades[ 2 - 1 ];
weight *= smoothstep(mix(previous.x, previous.y, 0.9), previous.y, linearDepth);
#endif
if (weight > 0.0 && linearDepth < 1.0) {
projected = directionalShadowMatrix[ 2 ] * vec4(vWorld, 1.0);
coord = projected.xyz / projected.w;
// The plane in map space: normals transform by the inverse
// transpose; its depth slope across the map is the gradient.
plane = cascadePlanes[ 2 ] * facet;
gradient = abs(plane.z) > 1e-12 ? -plane.xy / plane.z : vec2(0.0);
float sampleShadow = planeShadow(cascadeMaps[ 2 ], cascadeShadow.shadowMapSize, cascadeShadow.shadowBias, coord, gradient);
