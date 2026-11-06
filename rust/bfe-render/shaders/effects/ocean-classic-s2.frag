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
#define OPAQUE
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


  uniform sampler2D waterMap, foamMap, mask;
  uniform vec2 tile, foamTile, velocity, wind, gridOffset, fogRange;
  uniform float time, amplitude, cell, gridWidth;
  uniform vec3 fogColor, sunColor, sunDirection;
  varying vec3 vWorld;
  varying float vDepth;
  
  float wave(vec2 p) {
    vec2 across = vec2(-wind.y, wind.x);
    return sin(dot(p, wind) * 0.18 - time * 1.7) * 0.65
      + sin(dot(p, across) * 0.29 + time * 1.3) * 0.24
      + sin(dot(p, normalize(wind + across)) * 0.43 - time * 2.0) * 0.11;
  }

  void main() {
    vec2 maskUv = (vWorld.xz + gridOffset) / gridWidth;
    if(any(lessThan(maskUv,vec2(0.0))) || any(greaterThanEqual(maskUv,vec2(1.0))) || texture2D(mask,maskUv).r < 0.5) discard;
    vec2 uv = vWorld.xz / cell;
    vec2 drift = velocity * time;
    vec3 base = texture2D(waterMap,uv * tile + drift).rgb;
    base = mix(base,texture2D(waterMap,uv * tile * 1.63 - drift * 0.73).rgb,0.35);
    float h = wave(vWorld.xz);
    vec2 slope = vec2(wave(vWorld.xz + vec2(.2,0.0))-h,wave(vWorld.xz+vec2(0.0,.2))-h) * amplitude / .2;
    vec3 N = normalize(vec3(-slope.x,1.0,-slope.y));
    vec3 V = normalize(cameraPosition-vWorld), L = normalize(-sunDirection);
    float fresnel = pow(1.0-max(dot(N,V),0.0),4.0);
    vec3 color = base * (0.8 + 0.4 * max(N.y,0.0)) + vec3(.065,.085,.11) * fresnel;
    float crest = smoothstep(.58,.93,h) * .16;
    vec4 foam = texture2D(foamMap,uv * foamTile - drift);
    color = mix(color,foam.rgb,crest);
    color += sunColor * pow(max(dot(N,normalize(V+L)),0.0),48.0) * .2;
    float fog = smoothstep(fogRange.x,fogRange.y,vDepth);
    gl_FragColor = vec4(mix(color,fogColor,fog),1.0);
  }
