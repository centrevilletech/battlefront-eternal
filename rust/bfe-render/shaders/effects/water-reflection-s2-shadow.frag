#version 300 es
#define varying in
layout(location = 0) out highp vec4 pc_fragColor;
#define gl_FragColor pc_fragColor
#define gl_FragDepthEXT gl_FragDepth
#define texture2D texture
#define textureCube texture
#define texture2DProj textureProj
#define texture2DLodEXT textureLod
#define texture2DProjLodEXT textureProjLod
#define textureCubeLodEXT textureLod
#define texture2DGradEXT textureGrad
#define texture2DProjGradEXT textureProjGrad
#define textureCubeGradEXT textureGrad
precision highp float;
	precision highp int;
	precision highp sampler2D;
	precision highp samplerCube;
	precision highp sampler3D;
	precision highp sampler2DArray;
	precision highp sampler2DShadow;
	precision highp samplerCubeShadow;
	precision highp sampler2DArrayShadow;
	precision highp isampler2D;
	precision highp isampler3D;
	precision highp isamplerCube;
	precision highp isampler2DArray;
	precision highp usampler2D;
	precision highp usampler3D;
	precision highp usamplerCube;
	precision highp usampler2DArray;
	
#define HIGH_PRECISION
#define SHADER_TYPE ShaderMaterial
#define SHADER_NAME 
#define DOUBLE_SIDED
#define USE_SHADOWMAP
#define SHADOWMAP_TYPE_PCF_SOFT
uniform mat4 viewMatrix;
uniform vec3 cameraPosition;
uniform bool isOrthographic;
vec4 LinearTransferOETF( in vec4 value ) {
	return value;
}
vec4 sRGBTransferEOTF( in vec4 value ) {
	return vec4( mix( pow( value.rgb * 0.9478672986 + vec3( 0.0521327014 ), vec3( 2.4 ) ), value.rgb * 0.0773993808, vec3( lessThanEqual( value.rgb, vec3( 0.04045 ) ) ) ), value.a );
}
vec4 sRGBTransferOETF( in vec4 value ) {
	return vec4( mix( pow( value.rgb, vec3( 0.41666 ) ) * 1.055 - vec3( 0.055 ), value.rgb * 12.92, vec3( lessThanEqual( value.rgb, vec3( 0.0031308 ) ) ) ), value.a );
}
vec4 linearToOutputTexel( vec4 value ) {
	return LinearTransferOETF( vec4( value.rgb * mat3( 1.0000,-0.0000,-0.0000,-0.0000,1.0000,0.0000,0.0000,0.0000,1.0000 ), value.a ) );
}
float luminance( const in vec3 rgb ) {
	const vec3 weights = vec3( 0.2126, 0.7152, 0.0722 );
	return dot( weights, rgb );
}


  
  #ifdef FAR_WATER
    uniform vec2 surfaceRange;
    uniform vec3 fogColor;
    uniform vec2 fogRange;
    // The far scene's linear fog (materials.js FAR), 0 where there is none.
    float farFog(float depth) {
      return clamp((depth - fogRange.x) / (fogRange.y - fogRange.x), 0.0, 1.0);
    }
  #endif

  
  uniform vec4 waders[8];
  // Rings spreading from the legs of each wading soldier, as a slope.
  vec2 waderSlope(vec2 at, float time, out float near) {
    vec2 slope = vec2(0.0);
    near = 0.0;
    for (int i = 0; i < 8; i++) {
      if (waders[i].w <= 0.0) continue;
      vec2 delta = at - waders[i].xy;
      float d = length(delta);
      near = max(near, (1.0 - smoothstep(0.35, 0.8, d)) * waders[i].w);
      float wave = cos(d * 26.0 - time * 7.5) * exp(-max(d - 0.25, 0.0) * 2.2) * smoothstep(0.15, 0.3, d);
      slope += delta / max(d, 0.01) * wave * 0.09 * waders[i].w;
    }
    return slope;
  }

  uniform mat4 projectionMatrix;
  uniform sampler2D sceneDepth;
  uniform sampler2D sceneColor;
  uniform float screenSpace;
  uniform vec2 resolution;
  uniform sampler2D reflection;
  uniform mat4 reflectionMatrix;
  uniform sampler2D normalA;
  uniform sampler2D normalB;
  uniform float normalMix;
  uniform sampler2D bumpA;
  uniform sampler2D bumpB;
  uniform float bumpMix;
  uniform float distort;
  uniform float viewYaw;
  uniform vec2 tile;
  uniform vec3 toSun;
  uniform float reflectAmount;
  uniform vec2 fresnel;
  uniform float quality;
  uniform float time;
  uniform vec3 sunColor;
  uniform vec2 cameraRange;
  varying float vSurfaceDepth;
  varying vec3 vWorld;
  varying vec2 vUv;
  void main() {
    
    #ifdef FAR_WATER
      if (vSurfaceDepth < surfaceRange.x || vSurfaceDepth > surfaceRange.y) discard;
    #else
      if (vSurfaceDepth > cameraRange.y) discard;
    #endif
    gl_FragDepth = clamp((cameraRange.y - cameraRange.x * cameraRange.y / max(vSurfaceDepth, 0.0001))
      / (cameraRange.y - cameraRange.x), 0.0, 1.0);

    vec2 uv = vUv * tile;
    vec3 n = mix(texture2D(normalA, uv).rgb, texture2D(normalB, uv).rgb, normalMix) * 2.0 - 1.0;
    // The maps hold x in red, z in green, and up in blue.
    vec3 N = normalize(vec3(n.r, n.b, n.g));
    if (quality > 0.5) {
      // Cross-flowing world-space waves remove cell seams and visibly repeat
      // less than the original low-resolution flipbook on its own.
      vec2 flow = vWorld.xz * 0.22;
      vec3 detail = texture2D(normalA, flow + vec2(time * 0.018, -time * 0.011)).rgb * 2.0 - 1.0;
      vec3 crossFlow = texture2D(normalB, flow * 2.17 + vec2(-time * 0.013, time * 0.019)).rgb * 2.0 - 1.0;
      vec2 slope = vec2(
        cos(dot(vWorld.xz, vec2(0.72, 0.38)) + time * 0.95),
        cos(dot(vWorld.xz, vec2(-0.31, 1.13)) - time * 1.3)) * 0.075;
      float near;
      slope += waderSlope(vWorld.xz, time, near);
      N = normalize(vec3(N.xz * 0.35 + detail.rg * 0.22 + crossFlow.rg * 0.09 + slope, 1.0)).xzy;
    }
    vec3 V = normalize(cameraPosition - vWorld);
    vec3 H = normalize(V + toSun);
    float spec = pow(clamp(dot(N, H), 0.0, 1.0), 8.0);
    float grazing = 1.0 - clamp(dot(N, V), 0.0, 1.0);
    float F = fresnel.x + (fresnel.y - fresnel.x) * grazing * grazing;
    vec4 clip = reflectionMatrix * vec4(vWorld, 1.0);
    vec2 at = clip.xy / clip.w;
    if (distort > 0.0) {
      vec2 d = (mix(texture2D(bumpA, uv).rg, texture2D(bumpB, uv).rg, bumpMix) * 255.0 - 128.0) / 127.0;
      float c = cos(viewYaw);
      float s = sin(viewYaw);
      at += distort * vec2(c * d.x - s * d.y, s * d.x + c * d.y);
    }
    if (quality > 0.5) {
      // Fine normal distortion and energy-bounded highlights, with derivative
      // roughness to suppress shimmering subpixel sun glints in motion.
      at += N.xz * 0.012;
      float variance = dot(dFdx(N), dFdx(N)) + dot(dFdy(N), dFdy(N));
      float exponent = mix(90.0, 14.0, clamp(variance * 50.0, 0.0, 1.0));
      spec = pow(max(dot(N, H), 0.0), exponent) * 0.7;
      F = mix(max(0.04, fresnel.x * 0.4), fresnel.y, pow(grazing, 5.0));
    }
    vec3 mirrored = texture2D(reflection, clamp(at, vec2(0.002), vec2(0.998))).rgb;
    #ifdef FAR_WATER
    {
      // Past the near scene there is nothing near to reflect or meet.
      vec3 glint = quality > 0.5 ? sunColor * spec : vec3(spec);
      gl_FragColor = vec4(mix(mirrored * reflectAmount + glint, fogColor, farFog(vSurfaceDepth)), F);
      return;
    }
    #endif
    if (screenSpace > 0.5) {
      // High: the near scene comes from the opaque colour/depth copy the
      // water already takes, by a short march along the reflected ray in
      // view space, instead of a second (mirrored) render of the whole near
      // scene. Off-screen rays keep the far scene's planar mirror.
      vec3 viewPos = (viewMatrix * vec4(vWorld, 1.0)).xyz;
      vec3 ray = reflect(normalize(viewPos), normalize(mat3(viewMatrix) * N));
      vec3 p = viewPos;
      float stride = 0.2, found = 0.0;
      vec2 hit = vec2(0.0);
      for (int i = 0; i < 24; i++) {
        p += ray * stride;
        stride *= 1.28;
        vec4 clip = projectionMatrix * vec4(p, 1.0);
        if (clip.w <= 0.0) break;
        vec2 uv = clip.xy / clip.w * 0.5 + 0.5;
        if (any(lessThan(uv, vec2(0.0))) || any(greaterThan(uv, vec2(1.0)))) break;
        float stored = texture2D(sceneDepth, uv).r;
        float scene = cameraRange.x * cameraRange.y / (cameraRange.y - stored * (cameraRange.y - cameraRange.x));
        float depth = -p.z;
        if (depth > scene && depth - scene < stride * 1.6 + 0.25) {
          vec2 edge = min(uv, 1.0 - uv);
          found = smoothstep(0.0, 0.06, min(edge.x, edge.y)) * (1.0 - smoothstep(14.0, 24.0, float(i)));
          hit = uv;
          break;
        }
      }
      mirrored = mix(mirrored, texture2D(sceneColor, hit).rgb, found);
    }
    vec3 highlight = quality > 0.5 ? sunColor * spec : vec3(spec);
    if (quality > 0.5) {
      // Where a leg, wall or step meets the surface the mirror fades, so the
      // contact shows its foam and what's under it instead of a hard edge.
      float stored = texture2D(sceneDepth, gl_FragCoord.xy / resolution).r;
      float behind = cameraRange.x * cameraRange.y / (cameraRange.y - stored * (cameraRange.y - cameraRange.x));
      float meet = smoothstep(0.0, 0.18, behind - vSurfaceDepth);
      F *= meet;
      highlight *= meet;
    }
    gl_FragColor = vec4(mirrored * reflectAmount + highlight, F);
  }
