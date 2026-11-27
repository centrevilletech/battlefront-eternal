layout(location = 0) out highp vec4 pc_fragColor;

uniform mat4 viewMatrix;
uniform vec3 cameraPosition;
uniform vec3 sunDirection;
uniform float bump;
uniform vec3 sunColor;
uniform float shadowStrength;
uniform vec3 ambient;
uniform float aaSamples;
uniform vec3 ambientTop;
uniform float terrainShade;
uniform vec3 ambientBottom;
uniform float farAaSamples;
uniform vec3 characterAmbient;
uniform vec3 fogColor;
uniform float cascadeFar;
uniform vec2 fade;
uniform vec2 resolution;
uniform vec2 fogRange;

uniform float sunScale;
uniform float brightness;
#ifdef DYNAMIC
uniform vec2 ambientPull;
uniform vec4 localLightPositions[4];
uniform vec3 localLightColors[4];
uniform float localLightCount;
#if !defined(NO_POINT) && !defined(FAR)
uniform vec4 zeroLights[ZERO_LIGHTS * 2];
uniform int zeroLightCount;
#endif
#endif

#ifdef USE_SHADOWMAP
#if NUM_DIR_LIGHT_SHADOWS > 0
uniform bool receiveShadow;
struct DirectionalLightShadow {
  float shadowIntensity;
  float shadowBias;
  float shadowNormalBias;
  float shadowRadius;
  vec2 shadowMapSize;
};
uniform DirectionalLightShadow directionalLightShadows[NUM_DIR_LIGHT_SHADOWS];
uniform highp sampler2DShadow cascadeMaps[NUM_DIR_LIGHT_SHADOWS];
uniform mat4 directionalShadowMatrix[NUM_DIR_LIGHT_SHADOWS];
uniform mat3 cascadePlanes[NUM_DIR_LIGHT_SHADOWS];
uniform vec2 cascades[4];
#endif
#endif

#ifdef SPECULAR
uniform vec4 specular;
#endif
#ifdef USE_MAP
uniform sampler2D map;
#endif
#ifndef FAR
uniform sampler2D farMap;
#endif
#ifdef BUMP
uniform sampler2D bumpMap;
#endif
#ifdef TERRAIN
uniform vec2 layerScales;
uniform vec2 farOrigin;
uniform vec3 bakedLit;
#ifndef FAR
uniform sampler2D layer1Map;
uniform sampler2D detailMap;
#ifdef TERRAIN_LAYERS
uniform vec4 layerScale0;
uniform vec4 layerScale1;
uniform sampler2D layerMap0;
#if TERRAIN_LAYERS > 1
uniform sampler2D layerMap1;
#endif
#if TERRAIN_LAYERS > 2
uniform sampler2D layerMap2;
#endif
#if TERRAIN_LAYERS > 3
uniform sampler2D layerMap3;
#endif
#if TERRAIN_LAYERS > 4
uniform sampler2D layerMap4;
#endif
#if TERRAIN_LAYERS > 5
uniform sampler2D layerMap5;
#endif
#if TERRAIN_LAYERS > 6
uniform sampler2D layerMap6;
#endif
#if TERRAIN_LAYERS > 7
uniform sampler2D layerMap7;
#endif
#endif
#endif
#endif

in vec2 vUv;
in vec4 vColor;
in vec3 vNormalW;
in vec3 vWorld;
in vec3 vPoint;
in float vDepth;
#ifdef BUMP
in vec3 vTangent;
in vec3 vBinormal;
#endif
#ifdef TERRAIN_LAYERS
in vec4 vWeights0;
in vec4 vWeights1;
#endif

#ifdef TERRAIN_LAYERS
void paint(inout vec4 sum, sampler2D layer, float weight, float scale, vec2 ground, vec2 dx, vec2 dy) {
  if (weight > 0.002) sum += weight * textureGrad(layer, ground / scale, dx / scale, dy / scale);
}
vec4 paintedGround(vec2 ground) {
  vec2 dx = dFdx(ground);
  vec2 dy = dFdy(ground);
  vec4 sum = vec4(0.0);
  paint(sum, layerMap0, vWeights0.x, layerScale0.x, ground, dx, dy);
  #if TERRAIN_LAYERS > 1
    paint(sum, layerMap1, vWeights0.y, layerScale0.y, ground, dx, dy);
  #endif
  #if TERRAIN_LAYERS > 2
    paint(sum, layerMap2, vWeights0.z, layerScale0.z, ground, dx, dy);
  #endif
  #if TERRAIN_LAYERS > 3
    paint(sum, layerMap3, vWeights0.w, layerScale0.w, ground, dx, dy);
  #endif
  #if TERRAIN_LAYERS > 4
    paint(sum, layerMap4, vWeights1.x, layerScale1.x, ground, dx, dy);
  #endif
  #if TERRAIN_LAYERS > 5
    paint(sum, layerMap5, vWeights1.y, layerScale1.y, ground, dx, dy);
  #endif
  #if TERRAIN_LAYERS > 6
    paint(sum, layerMap6, vWeights1.z, layerScale1.z, ground, dx, dy);
  #endif
  #if TERRAIN_LAYERS > 7
    paint(sum, layerMap7, vWeights1.w, layerScale1.w, ground, dx, dy);
  #endif
  float total = dot(vWeights0, vec4(1.0)) + dot(vWeights1, vec4(1.0));
  return sum / max(total, 0.004);
}
#endif

#if defined(USE_SHADOWMAP) && NUM_DIR_LIGHT_SHADOWS > 0
float planeShadow(highp sampler2DShadow depthMap, vec2 size, float bias, vec3 coord, vec2 gradient) {
  if (coord.z < 0.0 || coord.z > 1.0 || any(lessThan(coord.xy, vec2(0.0))) || any(greaterThan(coord.xy, vec2(1.0)))) return 1.0;
  vec2 pixel = coord.xy * size - 0.5;
  vec2 base = floor(pixel);
  vec2 f = fract(pixel);
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

float sunShadow() {
  float shadow = 1.0;
  if (receiveShadow) {
    float linearDepth = vDepth / cascadeFar;
    vec3 facet = cross(dFdx(vWorld), dFdy(vWorld));
    #if NUM_DIR_LIGHT_SHADOWS > 0
    {
      DirectionalLightShadow cs = directionalLightShadows[0];
      vec2 interval = cascades[0];
      float weight = 1.0;
      #if 0 < NUM_DIR_LIGHT_SHADOWS - 1
        weight = 1.0 - smoothstep(mix(interval.x, interval.y, 0.9), interval.y, linearDepth);
      #endif
      if (weight > 0.0 && linearDepth < 1.0) {
        vec4 projected = directionalShadowMatrix[0] * vec4(vWorld, 1.0);
        vec3 coord = projected.xyz / projected.w;
        vec3 plane = cascadePlanes[0] * facet;
        vec2 gradient = abs(plane.z) > 1e-12 ? -plane.xy / plane.z : vec2(0.0);
        shadow -= (1.0 - planeShadow(cascadeMaps[0], cs.shadowMapSize, cs.shadowBias, coord, gradient)) * weight;
      }
    }
    #endif
    #if NUM_DIR_LIGHT_SHADOWS > 1
    {
      DirectionalLightShadow cs = directionalLightShadows[1];
      vec2 interval = cascades[1];
      vec2 prev = cascades[0];
      float weight = smoothstep(mix(prev.x, prev.y, 0.9), prev.y, linearDepth);
      #if 1 < NUM_DIR_LIGHT_SHADOWS - 1
        weight *= 1.0 - smoothstep(mix(interval.x, interval.y, 0.9), interval.y, linearDepth);
      #endif
      if (weight > 0.0 && linearDepth < 1.0) {
        vec4 projected = directionalShadowMatrix[1] * vec4(vWorld, 1.0);
        vec3 coord = projected.xyz / projected.w;
        vec3 plane = cascadePlanes[1] * facet;
        vec2 gradient = abs(plane.z) > 1e-12 ? -plane.xy / plane.z : vec2(0.0);
        shadow -= (1.0 - planeShadow(cascadeMaps[1], cs.shadowMapSize, cs.shadowBias, coord, gradient)) * weight;
      }
    }
    #endif
    #if NUM_DIR_LIGHT_SHADOWS > 2
    {
      DirectionalLightShadow cs = directionalLightShadows[2];
      vec2 interval = cascades[2];
      vec2 prev = cascades[1];
      float weight = smoothstep(mix(prev.x, prev.y, 0.9), prev.y, linearDepth);
      #if 2 < NUM_DIR_LIGHT_SHADOWS - 1
        weight *= 1.0 - smoothstep(mix(interval.x, interval.y, 0.9), interval.y, linearDepth);
      #endif
      if (weight > 0.0 && linearDepth < 1.0) {
        vec4 projected = directionalShadowMatrix[2] * vec4(vWorld, 1.0);
        vec3 coord = projected.xyz / projected.w;
        vec3 plane = cascadePlanes[2] * facet;
        vec2 gradient = abs(plane.z) > 1e-12 ? -plane.xy / plane.z : vec2(0.0);
        shadow -= (1.0 - planeShadow(cascadeMaps[2], cs.shadowMapSize, cs.shadowBias, coord, gradient)) * weight;
      }
    }
    #endif
    #if NUM_DIR_LIGHT_SHADOWS > 3
    {
      DirectionalLightShadow cs = directionalLightShadows[3];
      vec2 prev = cascades[2];
      float weight = smoothstep(mix(prev.x, prev.y, 0.9), prev.y, linearDepth);
      if (weight > 0.0 && linearDepth < 1.0) {
        vec4 projected = directionalShadowMatrix[3] * vec4(vWorld, 1.0);
        vec3 coord = projected.xyz / projected.w;
        vec3 plane = cascadePlanes[3] * facet;
        vec2 gradient = abs(plane.z) > 1e-12 ? -plane.xy / plane.z : vec2(0.0);
        shadow -= (1.0 - planeShadow(cascadeMaps[3], cs.shadowMapSize, cs.shadowBias, coord, gradient)) * weight;
      }
    }
    #endif
  }
  return mix(1.0, shadow, shadowStrength);
}
#else
float sunShadow() { return 1.0; }
#endif

#ifdef EXTENDED_LIGHT
vec3 bfeExtRegions(vec3 n, vec3 baseAmb, float shadow, int shadowReg, out float dir0Scale, out float dir1Scale, out vec3 regDirect, out vec3 regSpecDir, out vec3 regSpecCol) {
  dir0Scale = 1.0;
  dir1Scale = 1.0;
  regDirect = vec3(0.0);
  regSpecDir = vec3(0.0, -1.0, 0.0);
  regSpecCol = vec3(0.0);
  vec3 amb = baseAmb;
  int rCount = int(bfeLightMeta.x + 0.5);
  for (int r = 0; r < 4; r++) {
    if (r >= rCount) break;
    vec3 local = (bfeRegionInv[r] * vec4(vWorld, 1.0)).xyz;
    vec3 halfExt = bfeRegionHalf[r].xyz;
    float band = max(bfeRegionHalf[r].w, 0.01);
    float inside;
    if (halfExt.y < 0.0) {
      inside = halfExt.x - length(local);
    } else {
      vec3 d = halfExt - abs(local);
      inside = min(d.x, min(d.y, d.z));
    }
    float w = clamp(inside / band + 0.5, 0.0, 1.0);
    if (w > 0.0) {
      vec4 ambTop = bfeRegionAmbTop[r];
      vec4 ambBot = bfeRegionAmbBot[r];
      dir0Scale = min(dir0Scale, mix(1.0, ambTop.w, w));
      dir1Scale = min(dir1Scale, mix(1.0, ambBot.w, w));
      if (bfeRegionDirCol[r].w > 0.5) {
        vec3 rAmb = mix(ambTop.rgb, ambBot.rgb, 0.5 * (1.0 - n.y));
        amb = mix(amb, rAmb, w);
      }
      if (bfeRegionDir[r].w > 0.5) {
        float rl = max(dot(n, -bfeRegionDir[r].xyz), 0.0);
        float rShadow = (r == shadowReg) ? shadow : 1.0;
        regDirect += rl * bfeRegionDirCol[r].rgb * w * rShadow;
        regSpecDir = bfeRegionDir[r].xyz;
        regSpecCol += bfeRegionDirCol[r].rgb * w * rShadow;
      }
    }
  }
  return amb;
}
#endif

void main() {
  #ifdef BUMP
    vec2 bumpDx = dFdx(vUv);
    vec2 bumpDy = dFdy(vUv);
  #endif
  #ifdef FAR
    if (vDepth < fade.x) discard;
  #endif
  #ifdef TERRAIN
    vec2 ground = vWorld.xz;
    #ifdef FAR
      vec4 texel = texture(map, (ground - farOrigin) / layerScales);
    #else
      #ifdef TERRAIN_LAYERS
        vec4 texel = paintedGround(ground);
      #else
        vec4 texel = mix(texture(map, ground / layerScales.x), texture(layer1Map, ground / layerScales.y), vColor.a);
      #endif
      float detail = 1.0 - smoothstep(30.0, 80.0, vDepth);
      vec2 groundDx = dFdx(ground);
      vec2 groundDy = dFdy(ground);
      texel.rgb *= detail > 0.0 ? mix(vec3(0.5), textureGrad(detailMap, ground / 2.0, groundDx / 2.0, groundDy / 2.0).rgb, detail) * 2.0 : vec3(1.0);
    #endif
  #else
    #ifdef USE_MAP
      vec4 texel = texture(map, vUv);
    #else
      vec4 texel = vec4(1.0);
    #endif
  #endif

  #ifdef ALPHA_TEST
    float coverage = 1.0;
    #ifdef FAR
      #define BFE_SAMPLES farAaSamples
    #else
      #define BFE_SAMPLES aaSamples
    #endif
    #ifndef BLENDED
    if (BFE_SAMPLES > 0.0) {
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
  vec3 geoNormal = normalize(vNormalW);
  if (!gl_FrontFacing) geoNormal = -geoNormal;
  vec3 normal = geoNormal;
  float gloss = texel.a;
  #ifdef BUMP
    if (bump > 0.5) {
      vec4 bumpTexel = textureGrad(bumpMap, vUv, bumpDx, bumpDy);
      vec3 bn = bumpTexel.xyz * 2.0 - 1.0;
      normal = normalize(normalize(vTangent) * bn.x + normalize(vBinormal) * bn.y + normal * bn.z);
      gloss = bumpTexel.a;
    }
  #endif

  vec3 point = vPoint;
  #if defined(DYNAMIC) && !defined(NO_POINT) && !defined(FAR)
    point = vec3(0.0);
    for (int i = 0; i < ZERO_LIGHTS; i++) {
      if (i >= zeroLightCount) break;
      vec4 sphere = zeroLights[2 * i];
      vec3 delta = sphere.xyz - vWorld;
      float range = sphere.w;
      float d2 = dot(delta, delta);
      if (range <= 0.0 || d2 < range * range) {
        float reach = range > 0.0 ? max(1.0 - d2 / (range * range), 0.0) : 1.0;
        point += max(dot(normal, delta * inversesqrt(max(d2, 0.0001))), 0.0) * reach * zeroLights[2 * i + 1].rgb;
      }
    }
  #endif

  float dir0Scale = 1.0;
  float dir1Scale = 1.0;
  vec3 regSpecDir = vec3(0.0, -1.0, 0.0);
  vec3 regSpecCol = vec3(0.0);
  vec3 light;
  #if defined(GLOW)
    float glowBoost = 1.35;
    #ifdef EXTENDED_LIGHT
      glowBoost = bfeLightMeta.y > 0.0 ? bfeLightMeta.y : 1.35;
      #if !defined(ALPHA_TEST) && !defined(BLENDED)
        // The 2005 game brightens a glowing surface by 1 + a + a squared. An
        // opaque one reads a from its texture's alpha, a glow mask; one whose
        // alpha is its shape (cut out or blended) glows at the full a = 1.
        if (bfeLightMeta.y > 0.0) glowBoost = 1.0 + texel.a + texel.a * texel.a;
      #endif
    #endif
    light = tint * glowBoost;
  #elif defined(UNLIT)
    light = tint;
  #elif defined(TERRAIN)
    #ifdef FAR
      light = vColor.rgb * 2.0;
      #ifdef TERRAIN_RELIGHT
        if (terrainShade > 0.5) light *= 0.5 + 0.5 * clamp(1.0 + (dot(normal, -sunDirection) / max(-sunDirection.y, 0.25) - 1.0) * 2.2, 0.0, 1.3);
      #endif
    #else
      light = min(vColor.rgb, bakedLit * mix(0.5, 1.0, shadow)) * 2.0;
      #ifdef TERRAIN_RELIGHT
        if (terrainShade > 0.5) {
          float slope = clamp(1.0 + (dot(normal, -sunDirection) / max(-sunDirection.y, 0.25) - 1.0) * 2.2, 0.0, 1.3);
          float sun = 0.5 + 0.5 * slope * shadow;
          light = min(vColor.rgb * max(sun, 1.0), bakedLit * sun) * 2.0;
        }
      #endif
    #endif
  #elif defined(BAKED)
    light = clamp(vColor.rgb + vPoint, 0.0, 1.0);
  #else
    #ifdef DYNAMIC
      vec3 base = mix(ambientTop, ambientBottom, 0.5 * (1.0 - normal.y));
      base = mix(mix(base, vec3(0.0), ambientPull.x), vec3(1.0), ambientPull.y);
    #elif defined(EXTENDED_LIGHT)
      vec3 base = mix(ambientTop, ambientBottom, 0.5 * (1.0 - normal.y));
    #else
      vec3 base = ambient;
    #endif
    vec3 regDirect = vec3(0.0);
    int shadowReg = -1;
    #ifdef EXTENDED_LIGHT
      shadowReg = int(bfeLightMeta.z + 0.5) - 1;
      base = bfeExtRegions(normal, base, shadow, shadowReg, dir0Scale, dir1Scale, regDirect, regSpecDir, regSpecCol);
    #endif
    float sunShadowFactor = (shadowReg < 0) ? shadow : 1.0;
    float sunLit = dot(geoNormal, -sunDirection) > 0.0 ? max(dot(normal, -sunDirection), 0.0) : 0.0;
    vec3 direct = sunLit * sunColor * sunScale * sunShadowFactor * dir0Scale;
    #ifdef EXTENDED_LIGHT
      if (bfeBackLightDir.w > 0.5) {
        float backLit = dot(geoNormal, -bfeBackLightDir.xyz) > 0.0 ? max(dot(normal, -bfeBackLightDir.xyz), 0.0) : 0.0;
        direct += backLit * bfeBackLightColor.rgb * dir1Scale;
      }
      direct += regDirect;
    #endif
    light = clamp((base + direct + point) * tint, 0.0, 1.0);
  #endif

  vec4 color = vec4(texel.rgb * light * brightness, texel.a);
  // A glowing surface is unlit: lamps neither add to nor cap its glow.
  #if defined(DYNAMIC) && !defined(GLOW)
    vec3 lamp = vec3(0.0);
    for (int i = 0; i < 4; i++) {
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
    vec3 toEye = normalize(cameraPosition - vWorld);
    float specSunShadow = shadow;
    #ifdef EXTENDED_LIGHT
      if (bfeLightMeta.z > 0.5) specSunShadow = 1.0;
    #endif
    float highlight = pow(max(dot(reflect(sunDirection, normal), toEye), 0.0), specular.x);
    color.rgb += highlight * specular.yzw * sunColor * sunScale * specSunShadow * dir0Scale * gloss;
    #ifdef EXTENDED_LIGHT
      if (bfeBackLightDir.w > 0.5 && bfeBackLightColor.w > 0.0) {
        float bh = pow(max(dot(reflect(bfeBackLightDir.xyz, normal), toEye), 0.0), specular.x);
        color.rgb += bh * specular.yzw * bfeBackLightColor.rgb * bfeBackLightColor.w * dir1Scale * gloss;
      }
      if (any(greaterThan(regSpecCol, vec3(0.0)))) {
        float rh = pow(max(dot(reflect(regSpecDir, normal), toEye), 0.0), specular.x);
        color.rgb += rh * specular.yzw * regSpecCol * gloss;
      }
    #endif
  #endif

  #ifdef FAR
    float fog = clamp((vDepth - fogRange.x) / (fogRange.y - fogRange.x), 0.0, 1.0);
    #ifdef ADDITIVE
      color.rgb *= 1.0 - fog;
    #else
      color.rgb = mix(color.rgb, fogColor, fog);
    #endif
  #else
    float near = clamp((fade.y - vDepth) / max(fade.y - fade.x, 1.0), 0.0, 1.0);
    #ifdef BLENDED
      color.a *= near * near;
    #else
      if (near < 1.0) color.rgb = mix(textureLod(farMap, gl_FragCoord.xy / resolution, 0.0).rgb, color.rgb, near);
      color.a = 1.0;
    #endif
  #endif
  #if defined(ALPHA_TEST) && !defined(BLENDED)
    color.a = coverage;
  #endif
  pc_fragColor = color;
}
