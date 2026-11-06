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


  uniform sampler2D sceneColor;
  uniform sampler2D sceneDepth;
  uniform vec2 cameraRange;
  uniform vec3 tint;
  uniform float time;
  varying vec2 vUv;
  void main() {
    vec2 drift = vec2(sin(vUv.y * 25.0 + time * 0.7), cos(vUv.x * 21.0 + time * 0.55)) * 0.0007;
    vec2 at = clamp(vUv + drift, vec2(0.001), vec2(0.999));
    float z = texture2D(sceneDepth, at).r;
    float distance = cameraRange.x * cameraRange.y / (cameraRange.y - z * (cameraRange.y - cameraRange.x));
    vec3 source = texture2D(sceneColor, at).rgb;
    // Nearby surfaces remain legible; distant objects disappear into colored
    // scattering instead of applying the same dark tint at every distance.
    vec3 transmission = exp(-distance * vec3(0.105, 0.045, 0.065));
    vec3 scattering = tint * vec3(0.3, 0.55, 0.52);
    gl_FragColor = vec4(mix(scattering, source * vec3(0.86, 0.98, 0.93), transmission), 1.0);
  }
