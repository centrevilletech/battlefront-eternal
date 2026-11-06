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
#define SHADER_NAME storm-horizon
#define BLENDED 
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


 uniform sampler2D map;
 uniform vec3 hazeColor,lightningPosition;
 uniform float lightningFlash;
 varying vec2 vUv;
 varying vec3 vRay;
 void main(){
   vec4 texel=texture2D(map,vUv);
   #ifdef ALPHA_TEST
     if(texel.a<.5)discard;
   #endif
   float luma=dot(texel.rgb,vec3(.3,.59,.11));
   vec3 color=mix(texel.rgb,vec3(luma),.4)*vec3(.64,.74,.88)+texel.rgb*smoothstep(.5,.9,luma)*.7;
   vec2 strike=lightningPosition.xz-cameraPosition.xz;
   float bearing=max(dot(vRay.xz,strike),0.0)/max(length(vRay.xz)*length(strike),1e-4);
   float glow=pow(bearing,4.0);
   vec3 flash=vec3(.52,.64,.8)*lightningFlash;
   color+=texel.rgb*flash*(.3+glow*1.2);
   #ifdef ADDITIVE
     color*=1.0-.24;
   #else
     color=mix(color,hazeColor+flash*(.055+glow*.35),.24);
   #endif
   #ifdef BLENDED
     gl_FragColor=vec4(color,texel.a);
   #else
     gl_FragColor=vec4(color,1.0);
   #endif
 }
