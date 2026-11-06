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
#define USE_SHADOWMAP
#define SHADOWMAP_TYPE_PCF_SOFT
#define PREMULTIPLIED_ALPHA
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


uniform sampler2D volumeMap,sceneDepth;
uniform vec2 volumeResolution,depthResolution,cameraRange;
uniform float enabled;
float linearDepth(float z){return cameraRange.x*cameraRange.y/(cameraRange.y-z*(cameraRange.y-cameraRange.x));}
void main(){
  if(enabled<.5)discard;
  vec2 uv=gl_FragCoord.xy/depthResolution,pixel=uv*volumeResolution-.5,base=floor(pixel),f=fract(pixel);
  float depth=linearDepth(texture2D(sceneDepth,uv).r),total=0.;vec4 color=vec4(0.);
  for(int y=0;y<2;y++)for(int x=0;x<2;x++){
    vec2 offset=vec2(float(x),float(y)),at=(base+offset+.5)/volumeResolution;
    float neighbor=linearDepth(texture2D(sceneDepth,at).r);
    vec2 weight=mix(1.-f,f,offset);
    // Reject samples from the far side of silhouettes. Ordinary bilinear
    // filtering would smear the steam onto foreground actors and railings.
    float w=weight.x*weight.y*exp(-abs(depth-neighbor)*4./max(1.,depth*.02));
    color+=texture2D(volumeMap,at)*w;total+=w;
  }
  if(total<.001)discard;gl_FragColor=color/total;
}