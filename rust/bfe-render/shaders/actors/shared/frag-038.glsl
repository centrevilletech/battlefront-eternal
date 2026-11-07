// Shadows thin out toward the end of the last cascade.
shadow = mix(shadow, 1.0, smoothstep(0.85, 1.0, linearDepth));
}
#endif
return mix(1.0, shadow, shadowStrength);
}
void main() {
#ifdef SKY
// The dome is opaque; the models over it (mountains, a moon, cloud
// layers) keep their texture's alpha.
vec4 sky = texture2D(map, vUv);
#ifdef ALPHA_TEST
if (sky.a < 0.5) discard;
#endif
#ifdef BLENDED
gl_FragColor = sky;
#else
gl_FragColor = vec4(sky.rgb, 1.0);
#endif
#else
#ifdef BUMP
// The bump fetch's gradients, taken where every pixel still runs.
// The fetch sits behind the BUMPMAPPING switch, and Direct3D's
// compiler moves an implicit-gradient fetch out of a branch, so the
// normal map would be read with bump mapping off.
vec2 bumpDx = dFdx(vUv);
vec2 bumpDy = dFdy(vUv);
#endif
#ifdef FAR
// Reject the hidden near portion before any material texture fetches.
if (vDepth < fade.x) discard;
#endif
#ifdef TERRAIN
vec2 ground = vWorld.xz;
#ifdef FAR
vec4 texel = texture2D(map, (ground - farOrigin) / layerScales);
#else
#ifdef TERRAIN_LAYERS
vec4 texel = paintedGround(ground);
#else
vec4 texel = mix(texture2D(map, ground / layerScales.x), texture2D(layer1Map, ground / layerScales.y), vColor.a);
#endif
float detail = 1.0 - smoothstep(30.0, 80.0, vDepth);
// The detail texture is neutral past 80 m: no fetch there. Its
// gradients come from outside the branch, for a defined mip level.
vec2 groundDx = dFdx(ground);
vec2 groundDy = dFdy(ground);
texel.rgb *= detail > 0.0 ? mix(vec3(0.5), textureGrad(detailMap, ground / 2.0, groundDx / 2.0, groundDy / 2.0).rgb, detail) * 2.0 : vec3(1.0);
#endif
#else
vec4 texel = texture2D(map, vUv);
#endif
#ifdef ALPHA_TEST
// Coverage AA smooths texture silhouettes (fences/leaves), which
// ordinary MSAA cannot touch after a binary shader discard.
float coverage = 1.0;
#ifdef FAR
#define SAMPLES farAaSamples
#else
#define SAMPLES aaSamples
#endif
#ifndef BLENDED
if (SAMPLES > 0.0) {
float width = max(fwidth(texel.a), 0.0001);
coverage = smoothstep(0.5 - width * 0.5, 0.5 + width * 0.5, texel.a);
if (coverage <= 0.0) discard;
} else
#endif
if (texel.a * 255.0 <= 127.0) discard;
#endif
#ifdef FAR
float shadow = 1.0;
#else
float shadow = sunShadow();
#endif
vec3 tint = vec3(1.0);
#ifdef TINT
tint = vColor.rgb;
#endif
vec3 normal = normalize(vNormalW);
float gloss = texel.a;
vec3 point = vPoint;
#if defined( DYNAMIC ) && !defined( NO_POINT ) && !defined( FAR )
// Per pixel, the packed lights in world space (as in the vertex shader).
point = vec3(0.0);
for ( int i = 0; i < ZERO_LIGHTS; i ++ ) {
if ( i >= zeroLightCount ) break;
vec4 sphere = zeroLights[ 2 * i ];
vec3 delta = sphere.xyz - vWorld;
float range = sphere.w;
float distanceSquared = dot(delta, delta);
if (range <= 0.0 || distanceSquared < range * range) {
float reach = range > 0.0 ? max(1.0 - distanceSquared / (range * range), 0.0) : 1.0;
point += max(dot(normal, delta * inversesqrt(max(distanceSquared, 0.0001))), 0.0) * reach * zeroLights[ 2 * i + 1 ].rgb;
}
}
#endif
vec3 light;
#if defined( UNLIT )
light = tint;
#elif defined( TERRAIN )
#ifdef FAR
light = vColor.rgb * 2.0;
#ifdef TERRAIN_RELIGHT
// The same slope shading as the near ground, without shadows.
if (terrainShade > 0.5) light *= 0.5 + 0.5 * clamp(1.0 + (dot(normal, -sunDirection) / max(-sunDirection.y, 0.25) - 1.0) * 2.2, 0.0, 1.3);
#endif
#else
light = min(vColor.rgb, bakedLit * mix(0.5, 1.0, shadow)) * 2.0;
#ifdef TERRAIN_RELIGHT
// Enhanced shading for ground baked flat (no sun in its vertex
// colours, as on Hoth): the sun by the slope, scaled so open
// flat ground keeps its baked value and shadow stays at half.
// Slopes facing away fall toward the shadow level; slopes facing
// the sun brighten a little. Shaded snow takes the sky's chill.
if (terrainShade > 0.5) {
// Twice the slope's own change, so gentle drifts still read.
float slope = clamp(1.0 + (dot(normal, -sunDirection) / max(-sunDirection.y, 0.25) - 1.0) * 2.2, 0.0, 1.3);
float sun = 0.5 + 0.5 * slope * shadow;
light = min(vColor.rgb * max(sun, 1.0), bakedLit * sun) * 2.0;
float snow = smoothstep(0.62, 0.85, min(texel.r, min(texel.g, texel.b)));
light *= mix(vec3(1.0), vec3(0.9, 0.95, 1.05), snow * (1.0 - min(slope * shadow, 1.0)));
}
#endif
#endif
#elif defined( BAKED )
light = clamp(vColor.rgb + vPoint, 0.0, 1.0);
#else
light = clamp((vAmbient + vSun * shadow + point) * tint, 0.0, 1.0);
#ifdef BUMP
if (bump > 0.5) {
vec4 bumpTexel = textureGrad(bumpMap, vUv, bumpDx, bumpDy);
vec3 n = bumpTexel.xyz * 2.0 - 1.0;
normal = normalize(normalize(vTangent) * n.x + normalize(vBinormal) * n.y + normal * n.z);
gloss = bumpTexel.a;
// No sun at all on faces turned away from it.
float lit = dot(vNormalW, -sunDirection) > 0.0 ? max(dot(normal, -sunDirection), 0.0) : 0.0;
#ifdef DYNAMIC
vec3 base = characterAmbient;
base = mix(mix(base, vec3(0.0), ambientPull.x), vec3(1.0), ambientPull.y);
#else
vec3 base = ambient;
#endif
light = clamp(base + lit * sunColor * sunScale * shadow + point, 0.0, 1.0) * tint;
}
#endif
#endif
vec4 color = vec4(texel.rgb * light * brightness, texel.a);
#ifdef DYNAMIC
// Lamp illumination is per pixel and independent of baked terrain
// darkening, so crossing a light pool doesn't tint an entire triangle.
vec3 lamp = vec3(0.0);
for (int i = 0; i < 4; i++) {
// Unlit slots cost nothing: the count is a uniform.
if (float(i) >= localLightCount) break;
vec3 delta = localLightPositions[i].xyz - vWorld;
float d2 = dot(delta, delta);
float radius = max(localLightPositions[i].w, 0.001);
float reach = max(1.0 - d2 / (radius * radius), 0.0);
lamp += localLightColors[i] * reach * reach * max(dot(normal, delta * inversesqrt(max(d2, 0.0001))), 0.0);
}
color.rgb = texel.rgb * clamp(light * brightness + lamp * tint, 0.0, 1.0);
#endif
#ifdef SPECULAR
// The sun's highlight, reflected toward the eye.
vec3 toEye = normalize(cameraPosition - vWorld);
float highlight = pow(max(dot(reflect(sunDirection, normal), toEye), 0.0), specular.x);
color.rgb += highlight * specular.yzw * sunColor * sunScale * shadow * gloss;
#endif
