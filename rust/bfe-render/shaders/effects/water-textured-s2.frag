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

  uniform sampler2D mainMap, maskA, maskB;
  uniform float maskMix, time;
  uniform vec4 tint;
  uniform vec2 tile, velocity, maskTile, maskVelocity, cameraRange;
  uniform vec3 toSun;
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

    vec4 color = texture2D(mainMap, (vUv + fract(velocity * time)) * tile) * tint;
    vec2 uv = (vUv + fract(maskVelocity * time)) * maskTile;
    vec3 mask = mix(texture2D(maskA, uv).rgb, texture2D(maskB, uv).rgb, maskMix);
    vec3 halfVector = normalize(normalize(cameraPosition - vWorld) + toSun);
    color.rgb += mask * pow(max(halfVector.y, 0.0), 32.0);
    #ifdef FAR_WATER
      color.rgb = mix(color.rgb, fogColor, farFog(vSurfaceDepth));
    #endif
    gl_FragColor = color;
  }
