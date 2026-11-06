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


 uniform sampler3D volumeNoise;
 uniform vec2 wind;
 uniform vec3 eyePosition,hazeColor;
 uniform float time;
 varying vec2 vUv;
 float field(vec3 p){
   p.xz-=wind*time*7.0;
   vec3 q=p*.000065;
   float shape=texture(volumeNoise,q).r*.58+texture(volumeNoise,q*2.03).r*.28+texture(volumeNoise,q*4.11).r*.14;
   float height=(p.y-180.0)/460.0;
   float envelope=smoothstep(0.0,.18,height)*(1.0-smoothstep(.65,1.0,height));
   return max(0.0,shape-.48)*envelope*5.5;
 }
 void main(){
   float azimuth=(vUv.x-.5)*6.2831853,elevation=vUv.y*1.5707963;
   vec3 ray=vec3(sin(azimuth)*cos(elevation),sin(elevation),cos(azimuth)*cos(elevation));
   if(ray.y<.015){gl_FragColor=vec4(hazeColor,0);return;}
   float start=max(0.0,(180.0-eyePosition.y)/ray.y),end=max(0.0,(640.0-eyePosition.y)/ray.y);
   end=min(end,11000.0);
   float stepSize=max(1.0,(end-start)/28.0),transmission=1.0;vec3 color=vec3(0);
   // Fixed per-ray jitter removes marching bands without temporal grain.
   float jitter=fract(sin(dot(gl_FragCoord.xy,vec2(12.9898,78.233)))*43758.5453);
   for(int i=0;i<28;i++){
     vec3 p=eyePosition+ray*(start+(float(i)+jitter)*stepSize);
     float density=field(p);
     if(density>.005){
       float light=exp(-field(p+vec3(-80,130,60))*2.7);
       float top=clamp((p.y-180.0)/460.0,0.0,1.0);
       vec3 cloud=mix(vec3(.012,.018,.03),vec3(.26,.32,.39),pow(light,1.7)*.72+top*.18);
       float opacity=1.0-exp(-density*stepSize*.022);
       color+=transmission*opacity*cloud;transmission*=1.0-opacity;
       if(transmission<.015)break;
     }
   }
   vec3 sky=mix(hazeColor,vec3(.028,.042,.065),smoothstep(.0,.7,ray.y));
   color+=transmission*sky;
   float fog=1.0-exp(-start*.00014);
   color=mix(color,hazeColor,fog);
   gl_FragColor=vec4(color,1.0-transmission);
 }
