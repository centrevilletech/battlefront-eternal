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


  uniform float time;
  uniform float splash;
  varying vec2 vUv;
  varying float vPhase;
  varying float vPulse;
  varying vec3 vWorld;
  float hash(vec2 p) { return fract(sin(dot(p, vec2(127.1,311.7))) * 43758.5453); }
  void main() {
    float endFade = smoothstep(0.0, 0.16, vUv.x) * (1.0 - smoothstep(0.84, 1.0, vUv.x));
    float ripple = sin(vUv.y * 18.0 - time * 1.7 + vPhase * 6.283 + sin(vUv.x * 15.0) * 0.5);
    float flecks = hash(floor(vUv * vec2(55.0, 18.0) + vec2(time * 0.35, 0.0)));
    float band = pow(max(ripple, 0.0), 3.0) * (0.3 + 0.7 * flecks);
    float edgeFade = smoothstep(0.0, 0.08, vUv.y) * (1.0 - smoothstep(0.45, 1.0, vUv.y));
    float alpha = band * edgeFade * endFade * 0.38;
    if (splash > 0.5) {
      float column = floor(vUv.x * 13.0);
      float center = 0.2 + hash(vec2(column, vPhase)) * 0.5;
      vec2 drop = vec2((fract(vUv.x * 13.0) - 0.5) * 3.2, (vUv.y - center) * 8.0);
      float droplets = 1.0 - smoothstep(0.35, 1.0, length(drop));
      alpha = droplets * endFade * vPulse * 0.28;
    }
    alpha *= 1.0 - smoothstep(28.0, 40.0, distance(cameraPosition.xz, vWorld.xz));
    if (alpha < 0.006) discard;
    gl_FragColor = vec4(0.66, 0.76, 0.73, alpha);
  }
