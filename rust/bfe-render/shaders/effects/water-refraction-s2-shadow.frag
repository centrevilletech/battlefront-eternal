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

  uniform vec3 tint;
  uniform float quality;
  uniform sampler2D sceneColor;
  uniform sampler2D sceneDepth;
  uniform vec2 resolution;
  uniform vec2 cameraRange;
  uniform float underwater;
  uniform float time;
  varying float vSurfaceDepth;
  varying vec3 vWorld;
  float viewDepth(float depth) {
    return cameraRange.x * cameraRange.y / (cameraRange.y - depth * (cameraRange.y - cameraRange.x));
  }
  void main() {
    
    #ifdef FAR_WATER
      if (vSurfaceDepth < surfaceRange.x || vSurfaceDepth > surfaceRange.y) discard;
    #else
      if (vSurfaceDepth > cameraRange.y) discard;
    #endif
    gl_FragDepth = clamp((cameraRange.y - cameraRange.x * cameraRange.y / max(vSurfaceDepth, 0.0001))
      / (cameraRange.y - cameraRange.x), 0.0, 1.0);

    #ifdef FAR_WATER
    {
      // Past the near scene: what lies under it multiplied by the tint, or
      // (Enhanced) the colour the near pass gives water that deep, in the
      // fog (toward white where it multiplies).
      float fog = farFog(vSurfaceDepth);
      gl_FragColor = vec4(quality < 0.5 ? mix(tint, vec3(1.0), fog) : mix(tint * vec3(0.16, 0.36, 0.34), fogColor, fog), 1.0);
      return;
    }
    #endif
    if (quality < 0.5) { gl_FragColor = vec4(tint, 1.0); return; }
    vec2 screen = gl_FragCoord.xy / resolution;
    if (underwater > 0.5) {
      // The underside transmits the view above it; it must never reuse the
      // last above-water planar reflection when the eye crosses the surface.
      gl_FragColor = vec4(texture2D(sceneColor, screen).rgb * mix(vec3(1.0), tint, 0.25), 1.0);
      return;
    }
    float surface = max(vSurfaceDepth, 0.0001);
    float thickness = max(0.0, viewDepth(texture2D(sceneDepth, screen).r) - surface);
    vec2 bend = vec2(sin(vWorld.z * 1.2 + time), cos(vWorld.x * 0.9 - time * 0.8));
    vec2 refracted = clamp(screen + bend * 0.003 * min(thickness, 1.0), vec2(0.001), vec2(0.999));
    // Never pull an above-water object into the refracted silhouette.
    if (viewDepth(texture2D(sceneDepth, refracted).r) < surface) refracted = screen;
    vec3 transmission = exp(-thickness * vec3(0.22, 0.085, 0.13));
    vec3 deep = tint * vec3(0.16, 0.36, 0.34);
    vec3 color = mix(deep, texture2D(sceneColor, refracted).rgb, transmission);
    float caustic = pow(max(0.0, sin(vWorld.x * 2.5 + time) * sin(vWorld.z * 2.8 - time * 0.8)), 8.0);
    color += vec3(0.045, 0.07, 0.055) * caustic * exp(-thickness * 0.65);
    float edge = (1.0 - smoothstep(0.02, 0.55, thickness)) * smoothstep(0.0, 0.04, thickness);
    float foam = edge * (0.3 + 0.15 * sin(vWorld.x * 13.0 + vWorld.z * 11.0 + time * 1.7));
    color = mix(color, vec3(0.55, 0.64, 0.59), foam);
    // White water hugging whatever breaks the surface near a wading soldier
    // (his legs), broken up by moving noise, and the crests of his ripples.
    float near;
    vec2 ripple = waderSlope(vWorld.xz, time, near);
    float churn = 0.55 + 0.45 * sin(vWorld.x * 37.0 + time * 4.1) * sin(vWorld.z * 31.0 - time * 3.3);
    float hug = near * (1.0 - smoothstep(0.015, 0.14, thickness)) * smoothstep(0.0, 0.01, thickness);
    float crest = clamp(length(ripple) * 4.0, 0.0, 1.0) * near;
    color = mix(color, vec3(0.86, 0.92, 0.9), clamp(hug * churn * 1.1 + crest * 0.18, 0.0, 0.9));
    gl_FragColor = vec4(color, 1.0);
  }
