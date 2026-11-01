import * as THREE from "three";
import { localLighting } from "./local-lights.js";
import { ZERO_LIGHTS, lightMask, packed, pointLighting, sphereLightMask, useLights } from "./point-lights.js";

// Materials that light like the 2004 renderer: Direct3D 9 shaders working in
// gamma space, lighting per vertex. The light environments:
//
//   lit      props: the flat AmbientColor plus the sun, then the tint:
//            clamp((ambient + max(0, N.-sun) * sunColor + lights) * tint)
//   dynamic  soldiers and "Lighting dynamic" classes: the same with a
//            sky-to-ground ambient, lerp(top, bottom, (1 - N.y) / 2), and the
//            sun scaled by the light region the object stands in
//   baked    STATIC_LIT geometry: the vertex colour is the light (plus
//            lights, when the material is lit)
//   unlit    MTRL bit 0 clear: texture * tint
//   terrain  two layers by weight, the detail texture (0.5 is neutral), times
//            2 x the baked vertex colour
//   sky      the sky dome: the texture as it is
//
// Bump-mapped materials light the sun per pixel from a tangent-space normal
// map, sat(ambient + sat(n.L) * sunColor) * tint, using the model's own
// tangent and binormal (green = +v). Specular materials add a highlight of
// the sun scaled by a gloss alpha.
//
// The near scene dissolves into the far scene's image between the fade start
// and end. The far scene (FAR) is fogged, when the sky's fog is on (setFog),
// and leaves out whatever lies nearer than the fade start, where the near
// scene covers it.
//
// The sun's term is what the port's shadow maps darken. Baked terrain keeps
// its own shadows, and a shadow there only brings it down to the baked shadow
// strength (half), never below what is already baked.

// Shared by every material: the map's lights, the far scene, and the frame.
export const lighting = {
  sunDirection: { value: new THREE.Vector3(0, -1, 0) },
  sunColor: { value: new THREE.Vector3(0.5, 0.5, 0.5) },
  ambient: { value: new THREE.Vector3(0.5, 0.5, 0.5) },
  characterAmbient: { value: new THREE.Vector3(0.5, 0.5, 0.5) },
  ambientTop: { value: new THREE.Vector3(0.5, 0.5, 0.5) },
  ambientBottom: { value: new THREE.Vector3(0.5, 0.5, 0.5) },
  time: { value: 0 },
  fade: { value: new THREE.Vector2(300, 500) },
  farMap: { value: null },
  resolution: { value: new THREE.Vector2(1, 1) },
  fogColor: { value: new THREE.Vector3() },
  fogRange: { value: new THREE.Vector2(1e9, 2e9) },
  cascades: { value: [new THREE.Vector2(), new THREE.Vector2(), new THREE.Vector2(), new THREE.Vector2()] },
  cascadeNear: { value: 0.5 },
  cascadeFar: { value: 200 },
  // The cascades' depth maps (graphics.js), one a cascade, in the GPU's
  // own comparison mode.
  cascadeMaps: { value: [] },
  // Each cascade's world-to-map matrix as it applies to plane normals (the
  // inverse transpose of its rotation and scale), for the receiver plane.
  cascadePlanes: { value: [] },
  bump: { value: 1 },
  aaSamples: { value: 0 },
  farAaSamples: { value: 0 },
  shadowStrength: { value: 1 },
  // Enhanced terrain shading (Graphics > Terrain); see TERRAIN_RELIGHT.
  terrainShade: { value: 1 },
  // The point lights that reach what the draw covers (point-lights.js).
  zeroLights: pointLighting.drawLights,
  zeroLightCount: pointLighting.drawLightCount,
};

// Engine defaults when a map's sky leaves a light out.
export function setLighting(map) {
  lighting.shadowStrength.value = 1;
  const l = map.lighting || {};
  const set = (uniform, rgb, fallback) => {
    const c = rgb || fallback;
    uniform.value.set(c[0] / 255, c[1] / 255, c[2] / 255);
  };
  set(lighting.ambient, l.ambient, [128, 128, 128]);
  set(lighting.characterAmbient, l.character || l.ambient, [128, 128, 128]);
  set(lighting.ambientTop, l.top, [128, 128, 128]);
  set(lighting.ambientBottom, l.bottom, [128, 128, 128]);
  set(lighting.sunColor, l.sunColor, [128, 128, 128]);
  lighting.sunDirection.value.fromArray(l.sunDirection || [0, -1, 0]).normalize();
}

// The fog range that fogs nothing: it starts past any far scene.
export const NO_FOG = Object.freeze([1e9, 2e9]);

// Whether a fog range [start, end] fogs anything, as the 2004 game decides:
// it switches Direct3D's fog on only when 0 <= start < end, and its vertex
// shaders' fog factor is 1 (clear) when end <= start. Kashyyyk: Islands' sky
// sets 9000 and 8000, so its far scene draws unfogged.
export function fogs(range) {
  return Boolean(range) && range[0] >= 0 && range[0] < range[1];
}

// The far scene's fog (map.json distance.lowRes, from the sky's
// LowResTerrain): linear by view depth from FogNear (clear) to FogFar (the
// colour), FogFar already stretched by 255 / alpha at import. None without a
// LowResTerrain, or when its range fogs nothing. The colour is kept either
// way, as the original game keeps its render state.
export function setFog(map) {
  const low = map.distance?.lowRes;
  if (low?.fog) lighting.fogColor.value.set(low.fog[0] / 255, low.fog[1] / 255, low.fog[2] / 255);
  lighting.fogRange.value.fromArray(low?.fog && fogs(low.fogRange) ? low.fogRange : NO_FOG);
}

const vertexShader = /* glsl */ `
  #include <common>
  #include <skinning_pars_vertex>
  #include <lights_pars_begin>
  uniform vec3 sunDirection;
  uniform vec3 sunColor;
  uniform vec3 ambient;
  uniform vec3 ambientTop;
  uniform vec3 ambientBottom;
  uniform float sunScale;
  uniform vec2 ambientPull;
  uniform float time;
  uniform vec2 scroll;
  #if !defined( NO_POINT ) && !defined( DYNAMIC ) && !defined( FAR )
    uniform vec4 zeroLights[ZERO_LIGHTS * 2];
    uniform int zeroLightCount;
  #endif
  #ifdef HAS_COLOR
    attribute vec4 color;
  #endif
  #ifdef BUMP
    attribute vec3 tangent;
    attribute vec3 binormal;
    varying vec3 vTangent;
    varying vec3 vBinormal;
  #endif
  #ifdef TERRAIN_LAYERS
    attribute vec4 weights0;
    attribute vec4 weights1;
    varying vec4 vWeights0;
    varying vec4 vWeights1;
  #endif
  varying vec2 vUv;
  varying vec4 vColor;
  varying vec3 vAmbient;
  varying vec3 vSun;
  varying vec3 vPoint;
  varying vec3 vNormalW;
  varying vec3 vWorld;
  varying float vDepth;

  void main() {
    #ifdef TERRAIN_LAYERS
      vWeights0 = weights0;
      vWeights1 = weights1;
    #endif
    #include <beginnormal_vertex>
    #ifdef WORLD_SPACE
      // The map's own meshes (world.js): vertices already in the world and
      // a model matrix of identity, so the model-view matrix is the view
      // matrix itself and the model and normal matrices are skipped. Older
      // GPUs read every uniform a vertex uses from memory, a read each row.
      #include <begin_vertex>
      vec4 mvPosition = viewMatrix * vec4( transformed, 1.0 );
      gl_Position = projectionMatrix * mvPosition;
    #elif defined( USE_SKINNING ) && defined( RIGID_SKIN )
      // One bone a vertex at full weight and a bind matrix of identity (the
      // 2004 game's soldiers, actor.js). three's blend of four bones reduces
      // to this term for term: the other weights are zero and multiplying by
      // the identity bind matrix changes nothing. One bone's matrix is read
      // instead of four (each four texel reads) and two matrix products go.
      mat4 boneMatX = getBoneMatrix( skinIndex.x );
      mat4 skinMatrix = bindMatrixInverse * ( skinWeight.x * boneMatX );
      objectNormal = vec4( skinMatrix * vec4( objectNormal, 0.0 ) ).xyz;
      #include <defaultnormal_vertex>
      #include <begin_vertex>
      transformed = ( bindMatrixInverse * ( boneMatX * vec4( transformed, 1.0 ) * skinWeight.x ) ).xyz;
      #include <project_vertex>
      #include <worldpos_vertex>
    #else
      #include <skinbase_vertex>
      #include <skinnormal_vertex>
      #include <defaultnormal_vertex>
      #include <begin_vertex>
      #include <skinning_vertex>
      #include <project_vertex>
      #include <worldpos_vertex>
    #endif

    // Scrolling materials move at -speed * t, in repeats per second.
    vUv = uv;
    #ifdef SCROLL
      vUv -= scroll * time;
    #endif
    #ifdef HAS_COLOR
      vColor = color;
    #else
      vColor = vec4(1.0);
    #endif
    #ifdef WORLD_SPACE
      vec3 n = normalize(objectNormal);
      vNormalW = n;
      vWorld = transformed;
    #else
      // transformedNormal includes skinning and the inverse-transpose normal
      // matrix. A plain modelMatrix multiplication breaks nonuniform scales.
      vec3 n = inverseTransformDirection(transformedNormal, viewMatrix);
      vNormalW = n;
      vWorld = (modelMatrix * vec4(transformed, 1.0)).xyz;
    #endif
    vDepth = -mvPosition.z;
    #ifdef DYNAMIC
      vAmbient = mix(ambientTop, ambientBottom, 0.5 * (1.0 - n.y));
      // A light region pulls a dynamic object's ambient toward black or white.
      vAmbient = mix(mix(vAmbient, vec3(0.0), ambientPull.x), vec3(1.0), ambientPull.y);
    #else
      vAmbient = ambient;
    #endif
    vSun = max(dot(n, -sunDirection), 0.0) * sunColor * sunScale;
    #ifdef BUMP
      vec3 tangentObject = tangent;
      vec3 binormalObject = binormal;
      #ifdef USE_SKINNING
        tangentObject = mat3(skinMatrix) * tangentObject;
        binormalObject = mat3(skinMatrix) * binormalObject;
      #endif
      #ifdef WORLD_SPACE
        vTangent = tangentObject;
        vBinormal = binormalObject;
      #else
        vTangent = mat3(modelMatrix) * tangentObject;
        vBinormal = mat3(modelMatrix) * binormalObject;
      #endif
    #endif
    // Point lights: max(0, N.toLight) * (1 - d^2 / r^2) * colour, for the
    // lights that are on and within reach of what is drawn (point-lights.js),
    // in world space. Past its range a light adds nothing: its colour isn't
    // even read.
    vPoint = vec3(0.0);
    #if !defined( NO_POINT ) && !defined( DYNAMIC ) && !defined( FAR )
      for ( int i = 0; i < ZERO_LIGHTS; i ++ ) {
        if ( i >= zeroLightCount ) break;
        vec4 sphere = zeroLights[ 2 * i ];
        vec3 toLight = sphere.xyz - vWorld;
        float range = sphere.w;
        float distanceSquared = dot(toLight, toLight);
        if (range <= 0.0 || distanceSquared < range * range) {
          float reach = range > 0.0 ? max(1.0 - distanceSquared / (range * range), 0.0) : 1.0;
          vPoint += max(dot(n, normalize(toLight)), 0.0) * reach * zeroLights[ 2 * i + 1 ].rgb;
        }
      }
    #endif
  }
`;

const fragmentShader = /* glsl */ `
  #include <common>
  #include <packing>
  #include <lights_pars_begin>
  #include <shadowmap_pars_fragment>
  uniform sampler2D map;
  uniform vec3 sunDirection;
  uniform vec3 sunColor;
  uniform vec3 characterAmbient;
  uniform vec3 ambient;
  uniform float sunScale;
  uniform float brightness;
  uniform vec2 ambientPull;
  #ifdef DYNAMIC
    uniform vec4 localLightPositions[4];
    uniform vec3 localLightColors[4];
    uniform float localLightCount;
    #if !defined( NO_POINT ) && !defined( FAR )
      uniform vec4 zeroLights[ZERO_LIGHTS * 2];
      uniform int zeroLightCount;
    #endif
  #endif
  uniform vec2 fade;
  uniform sampler2D farMap;
  uniform vec2 resolution;
  uniform vec3 fogColor;
  uniform vec2 fogRange;
  uniform vec2 cascades[4];
  uniform float cascadeNear;
  uniform float cascadeFar;
  uniform float bump;
  uniform float aaSamples;
  uniform float farAaSamples;
  uniform float shadowStrength;
  uniform float terrainShade;
  uniform vec4 specular;
  #ifdef BUMP
    uniform sampler2D bumpMap;
    varying vec3 vTangent;
    varying vec3 vBinormal;
  #endif
  #ifdef TERRAIN
    uniform sampler2D layer1Map;
    uniform sampler2D detailMap;
    uniform vec2 layerScales;
    uniform vec2 farOrigin;
    uniform vec3 bakedLit;
  #endif
  #ifdef TERRAIN_LAYERS
    // Every map layer a patch paints with, summed by per-vertex weight.
    uniform vec4 layerScale0;
    uniform vec4 layerScale1;
    varying vec4 vWeights0;
    varying vec4 vWeights1;
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
    // Gradients come from the shared world position, so a layer can be
    // skipped where it has no weight without breaking its mip selection.
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
  varying vec2 vUv;
  varying vec4 vColor;
  varying vec3 vAmbient;
  varying vec3 vSun;
  varying vec3 vPoint;
  varying vec3 vNormalW;
  varying vec3 vWorld;
  varying float vDepth;

  // Receiver-plane PCF compares each tap against the depth of THIS triangle
  // at that tap. A constant normal offset can smear low-poly animated faces
  // and is too small for distant cascades. The footprint is a 4x4 tent: the
  // texels base-1 to base+2 on each axis, weighted (1-f, 1, 1, f)/3, which
  // is a 3x3 box of bilinear taps. The GPU's comparison sampler returns a
  // bilinear blend of four texel comparisons, so two taps an axis (weights
  // 2-f and 1+f, placed where their bilinear blends sum to the tent) make
  // the same sixteen comparisons in four fetches. Each fetch compares its
  // four texels against the receiver plane at the fetch.
  #if defined( USE_SHADOWMAP ) && NUM_DIR_LIGHT_SHADOWS > 0
  uniform highp sampler2DShadow cascadeMaps[ NUM_DIR_LIGHT_SHADOWS ];
  // The map coordinates come from the world position here, for the one or
  // two cascades a pixel reads, instead of four vec4 varyings a vertex.
  uniform mat4 directionalShadowMatrix[ NUM_DIR_LIGHT_SHADOWS ];
  uniform mat3 cascadePlanes[ NUM_DIR_LIGHT_SHADOWS ];
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
    #if defined( USE_SHADOWMAP ) && NUM_DIR_LIGHT_SHADOWS > 0
      if (receiveShadow) {
        // CSM's split fractions are distances divided by far, not far-near.
        float linearDepth = vDepth / cascadeFar;
        // The receiver plane is the triangle's: its normal from the world
        // position's derivatives, taken here, outside the per-pixel cascade
        // branches, where every fragment of the quad contributes.
        vec3 facet = cross(dFdx(vWorld), dFdy(vWorld));
        DirectionalLightShadow cascadeShadow;
        vec2 interval;
        vec2 previous;
        float weight;
        vec4 projected;
        vec3 coord;
        vec3 plane;
        vec2 gradient;
        #pragma unroll_loop_start
        for ( int i = 0; i < NUM_DIR_LIGHT_SHADOWS; i ++ ) {
          cascadeShadow = directionalLightShadows[ i ];
          interval = cascades[ UNROLLED_LOOP_INDEX ];
          weight = 1.0;
          #if UNROLLED_LOOP_INDEX < NUM_DIR_LIGHT_SHADOWS - 1
            weight = 1.0 - smoothstep(mix(interval.x, interval.y, 0.9), interval.y, linearDepth);
          #endif
          #if UNROLLED_LOOP_INDEX > 0
            previous = cascades[ UNROLLED_LOOP_INDEX - 1 ];
            weight *= smoothstep(mix(previous.x, previous.y, 0.9), previous.y, linearDepth);
          #endif
          if (weight > 0.0 && linearDepth < 1.0) {
            projected = directionalShadowMatrix[ i ] * vec4(vWorld, 1.0);
            coord = projected.xyz / projected.w;
            // The plane in map space: normals transform by the inverse
            // transpose; its depth slope across the map is the gradient.
            plane = cascadePlanes[ i ] * facet;
            gradient = abs(plane.z) > 1e-12 ? -plane.xy / plane.z : vec2(0.0);
            float sampleShadow = planeShadow(cascadeMaps[ i ], cascadeShadow.shadowMapSize, cascadeShadow.shadowBias, coord, gradient);
            shadow -= (1.0 - sampleShadow) * weight;
          }
        }
        #pragma unroll_loop_end
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

      #ifdef FAR
        // Linear fog by depth; additive surfaces fog toward black.
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
          // Most pixels are fully near: don't spend a far-scene texture read
          // on every fragment unless this pixel is actually in the dissolve.
          // An explicit level (the image has one) keeps the read inside the
          // branch on Direct3D, whose compiler hoists implicit-gradient ones.
          if (near < 1.0) color.rgb = mix(textureLod(farMap, gl_FragCoord.xy / resolution, 0.0).rgb, color.rgb, near);
          color.a = 1.0;
        #endif
      #endif
      #if defined( ALPHA_TEST ) && !defined( BLENDED )
        color.a = coverage;
      #endif
      gl_FragColor = color;
    #endif
  }
`;

// A material for one of the map's meshes, or for the soldier.
//   kind: "lit" | "dynamic" | "baked" | "unlit" | "terrain" | "sky"
//   options: map, bumpMap, layer1Map, detailMap, layerScales, farOrigin, bakedLit,
//   colored and tint (a vertex colour, and whether it is a tint), lights
//   (takes point lights), scroll [u, v], specular [exponent, r, g, b],
//   alphaTest, blending ("normal" | "additive" | null), doubleSided, far.
export function zeroMaterial(kind, options = {}) {
  const defines = { ZERO_LIGHTS };
  if (kind === "dynamic") defines.DYNAMIC = "";
  if (kind === "baked") defines.BAKED = "";
  if (kind === "unlit") defines.UNLIT = "";
  if (kind === "terrain") defines.TERRAIN = "";
  if (kind === "sky") defines.SKY = "";
  if (options.colored || kind === "terrain") defines.HAS_COLOR = "";
  if (options.tint) defines.TINT = "";
  if (options.lights === false) defines.NO_POINT = "";
  if (options.bumpMap && kind !== "terrain") defines.BUMP = "";
  if (options.scroll) defines.SCROLL = "";
  if (options.specular && options.specular.slice(1).some((c) => c > 0)) defines.SPECULAR = "";
  if (options.alphaTest) defines.ALPHA_TEST = "";
  if (options.blending) defines.BLENDED = "";
  if (options.blending === "additive") defines.ADDITIVE = "";
  if (options.far) defines.FAR = "";
  const [exponent = 0, r = 0, g = 0, b = 0] = options.specular || [];
  const uniforms = {
    ...THREE.UniformsUtils.clone(THREE.UniformsLib.lights),
    map: { value: options.map || null },
    bumpMap: { value: options.bumpMap || null },
    layer1Map: { value: options.layer1Map || options.map || null },
    detailMap: { value: options.detailMap || neutralTexture() },
    layerScales: { value: new THREE.Vector2(...(options.layerScales || [8, 8])) },
    // Where the far scene's low-res texture starts (the terrain's corner).
    farOrigin: { value: new THREE.Vector2(...(options.farOrigin || [0, 0])) },
    layerScale0: { value: new THREE.Vector4(8, 8, 8, 8) },
    layerScale1: { value: new THREE.Vector4(8, 8, 8, 8) },
    bakedLit: { value: new THREE.Vector3(...(options.bakedLit || [145 / 255, 120 / 255, 90 / 255])) },
    scroll: { value: new THREE.Vector2(...(options.scroll || [0, 0])) },
    specular: { value: new THREE.Vector4(THREE.MathUtils.clamp(exponent, 10, 100), r, g, b) },
    sunScale: { value: 1 },
    ambientPull: { value: new THREE.Vector2() },
    brightness: { value: 1 },
    localLightPositions: localLighting.positions,
    localLightColors: localLighting.colors,
    localLightCount: localLighting.count,
  };
  // The shared uniforms are shared objects: one change reaches every material.
  Object.assign(uniforms, lighting);
  const blended = Boolean(options.blending);
  const material = new THREE.ShaderMaterial({
    name: `zero:${kind}`,
    defines,
    uniforms,
    vertexShader,
    fragmentShader,
    lights: true,
    transparent: blended,
    depthWrite: !blended && kind !== "sky",
    depthTest: kind !== "sky",
    blending: options.blending === "additive" ? THREE.AdditiveBlending : THREE.NormalBlending,
    side: options.doubleSided ? THREE.DoubleSide : THREE.FrontSide,
    alphaToCoverage: Boolean(options.alphaTest) && !blended,
  });
  // WebGLShadowMap reads these material properties, not shader uniforms or
  // custom defines. Without them fences/leaves cast solid polygon shadows.
  material.map = options.map || null;
  material.alphaTest = options.alphaTest ? 0.5 : 0;
  material.shadowSide = options.doubleSided ? THREE.DoubleSide : THREE.FrontSide;
  // Whether its shaders add point lights at all (unlit, terrain, sky and the
  // far scene's don't).
  material.userData.pointLit = !options.far && options.lights !== false && (kind === "lit" || kind === "dynamic" || kind === "baked");
  material.onBeforeRender = (renderer, scene, camera, geometry, object) => {
    if (kind === "dynamic") {
      localLighting.select(object, scene.userData.isViewScene);
      material.uniformsNeedUpdate = true;
    }
    // The map's culled buckets chose theirs by the cells they draw
    // (render-culling.js).
    if (material.userData.pointLit && !object.userData.cellLights) selectLights(material, object, geometry, scene, camera);
  };
  return material;
}

// The point lights a zero material's draw reads (every draw of one sets
// them, the derived ones too: destruction-debris.js): the scene's that
// reach the object, by its bounds in the world. A soldier's body has the
// pose sphere actor.js gives it, which any pose stays inside; other meshes
// their geometry's box. Instanced meshes, and skinned ones without such a
// sphere, draw outside their geometry's bounds and keep every light.
const maskBox = new THREE.Box3(), maskCenter = new THREE.Vector3(), maskBoxes = [maskBox];
const IDENTITY = new THREE.Matrix4();
export function selectLights(material, object, geometry, scene, camera) {
  const pack = packed(scene, camera);
  let mask = -1;
  if (object.isSkinnedMesh) {
    const sphere = object.boundingSphere;
    if (sphere && object.userData.poseSphere) {
      maskCenter.copy(sphere.center).applyMatrix4(object.matrixWorld);
      mask = sphereLightMask(pack, maskCenter, sphere.radius * object.matrixWorld.getMaxScaleOnAxis());
    }
  } else if (!object.isInstancedMesh) {
    if (!geometry.boundingBox) geometry.computeBoundingBox();
    maskBox.copy(geometry.boundingBox);
    if (!object.matrixWorld.equals(IDENTITY)) maskBox.applyMatrix4(object.matrixWorld);
    mask = lightMask(pack, maskBoxes);
  }
  setLightMask(material, pack, mask);
}

export function setLightMask(material, pack, mask) {
  useLights(pack, mask);
  // The same material can draw again with other lights (a reflection, a
  // mirrored view): upload this draw's.
  material.uniformsNeedUpdate = true;
}

let neutral = null;
function neutralTexture() {
  if (!neutral) {
    neutral = new THREE.DataTexture(new Uint8Array([128, 128, 128, 255]), 1, 1);
    neutral.needsUpdate = true;
  }
  return neutral;
}

// How MTRL flags blend: 0x80 adds, 0x4 lays the texture over by its alpha.
// A hard-edged (0x2) material that doesn't add is opaque: the Normal, Bump,
// Scroll and Detail shaders clear its 0x4 when they load it, so it is cut
// out at half alpha, writes depth and draws with the opaque geometry. Every
// stock hard-edged material uses one of those render types (only Specmap
// would keep the blend).
export function blendingOf(flags) {
  if (flags & 0x80) return "additive";
  return flags & 0x4 && !(flags & 0x2) ? "normal" : null;
}

// The material kind and options for a mesh record from map.json.
export function describe(record) {
  const flags = record.flags || 0;
  let kind = "lit";
  if (record.sky) kind = "sky";
  else if (record.terrain) kind = "terrain";
  else if (record.colorKind === "lit" || flags & 0x40) kind = "baked";
  else if (!(flags & 0x1)) kind = "unlit";
  else if (record.dynamic) kind = "dynamic";
  return {
    kind,
    colored: Boolean(record.colorKind),
    tint: record.colorKind === "tint",
    lights: Boolean(flags & 0x1),
    scroll: record.scroll,
    specular: flags & 0x20 ? record.specular : null,
    alphaTest: Boolean(flags & 0x2),
    blending: blendingOf(flags),
    doubleSided: Boolean(flags & 0x86),
    far: Boolean(record.far),
  };
}
