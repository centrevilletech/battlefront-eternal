#version 300 es

#define attribute in
#define varying out
#define texture2D texture
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
#define STORM 
#define DOUBLE_SIDED
uniform mat4 modelMatrix;
uniform mat4 modelViewMatrix;
uniform mat4 projectionMatrix;
uniform mat4 viewMatrix;
uniform mat3 normalMatrix;
uniform vec3 cameraPosition;
uniform bool isOrthographic;
#ifdef USE_INSTANCING
	attribute mat4 instanceMatrix;
#endif
#ifdef USE_INSTANCING_COLOR
	attribute vec3 instanceColor;
#endif
#ifdef USE_INSTANCING_MORPH
	uniform sampler2D morphTexture;
#endif
attribute vec3 position;
attribute vec3 normal;
attribute vec2 uv;
#ifdef USE_UV1
	attribute vec2 uv1;
#endif
#ifdef USE_UV2
	attribute vec2 uv2;
#endif
#ifdef USE_UV3
	attribute vec2 uv3;
#endif
#ifdef USE_TANGENT
	attribute vec4 tangent;
#endif
#if defined( USE_COLOR_ALPHA )
	attribute vec4 color;
#elif defined( USE_COLOR )
	attribute vec3 color;
#endif
#ifdef USE_SKINNING
	attribute vec4 skinIndex;
	attribute vec4 skinWeight;
#endif


 
 uniform vec2 wind;
 uniform float time, stormStrength;
 void swell(vec2 p, float t, vec2 dir, float length, float amp, float steep,
            inout vec3 offset, inout vec3 dx, inout vec3 dz, inout float compression) {
   float k=6.2831853/length, phase=k*dot(dir,p)-sqrt(9.81*k)*t;
   float range=distance(p,cameraPosition.xz);
   float spacing=mix(3.0,range*.05,smoothstep(170.0,210.0,range));
   float resolved=1.0-smoothstep(length*.18,length*.5,spacing);
   float s=sin(phase), c=cos(phase), a=amp*stormStrength*resolved, q=steep*stormStrength*resolved;
   offset+=vec3(dir.x*q/k*c,a*s,dir.y*q/k*c);
   dx+=vec3(-dir.x*dir.x*q*s,dir.x*k*a*c,-dir.x*dir.y*q*s);
   dz+=vec3(-dir.x*dir.y*q*s,dir.y*k*a*c,-dir.y*dir.y*q*s);
   compression+=q*s;
 }
 void sea(vec2 p,float t,out vec3 offset,out vec3 normal,out float crest) {
   vec3 dx=vec3(1,0,0), dz=vec3(0,0,1);offset=vec3(0);crest=0.0;
   vec2 across=vec2(-wind.y,wind.x);
   swell(p,t,wind,78.0,3.2,.28,offset,dx,dz,crest);
   swell(p,t,normalize(wind+across*.52),43.0,1.9,.23,offset,dx,dz,crest);
   swell(p,t,normalize(wind-across*.65),27.0,1.05,.17,offset,dx,dz,crest);
   swell(p,t,normalize(wind+across*1.3),15.0,.52,.095,offset,dx,dz,crest);
   swell(p,t,normalize(wind-across*.3),8.1,.23,.045,offset,dx,dz,crest);
   normal=normalize(cross(dz,dx));
 }

 varying vec3 vWorld,vNormal;
 varying vec2 vRest;
 varying float vCrest;
 void main(){
   vec3 world=(modelMatrix*vec4(position,1)).xyz;
   vRest=world.xz;vec3 offset;
   sea(world.xz,time,offset,vNormal,vCrest);
   // The unresolved outermost rings disappear in sea haze, without a hard seam.
   offset*=1.0-smoothstep(1400.0,3500.0,length(position.xz));
   world+=offset;vWorld=world;
   gl_Position=projectionMatrix*viewMatrix*vec4(world,1);
 }
