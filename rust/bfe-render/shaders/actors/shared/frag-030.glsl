weight = 1.0 - smoothstep(mix(interval.x, interval.y, 0.9), interval.y, linearDepth);
#endif
#if 0 > 0
previous = cascades[ 0 - 1 ];
weight *= smoothstep(mix(previous.x, previous.y, 0.9), previous.y, linearDepth);
#endif
if (weight > 0.0 && linearDepth < 1.0) {
projected = directionalShadowMatrix[ 0 ] * vec4(vWorld, 1.0);
coord = projected.xyz / projected.w;
// The plane in map space: normals transform by the inverse
// transpose; its depth slope across the map is the gradient.
plane = cascadePlanes[ 0 ] * facet;
gradient = abs(plane.z) > 1e-12 ? -plane.xy / plane.z : vec2(0.0);
float sampleShadow = planeShadow(cascadeMaps[ 0 ], cascadeShadow.shadowMapSize, cascadeShadow.shadowBias, coord, gradient);
shadow -= (1.0 - sampleShadow) * weight;
}
cascadeShadow = directionalLightShadows[ 1 ];
interval = cascades[ 1 ];
weight = 1.0;
