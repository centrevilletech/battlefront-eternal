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
#define USE_SHADOWMAP
#define SHADOWMAP_TYPE_PCF_SOFT
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

 
 float hash(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}
 float noise(vec2 p){vec2 i=floor(p),f=fract(p);f=f*f*(3.0-2.0*f);
   return mix(mix(hash(i),hash(i+vec2(1,0)),f.x),mix(hash(i+vec2(0,1)),hash(i+1.0),f.x),f.y);}

 
 uniform sampler2D mask;
 uniform vec2 gridOffset;
 uniform float gridWidth;
 float wet(vec2 p){vec2 uv=(p+gridOffset)/gridWidth;
   if(any(lessThan(uv,vec2(0)))||any(greaterThanEqual(uv,vec2(1))))return 0.0;
   return step(.5,texture2D(mask,uv).r);}

 attribute vec4 seed;
 uniform vec2 eye;
 uniform float seaLevel;
 varying vec2 vUv;
 varying float vAlpha,vDepth,vMist;
 void main(){
   vUv=uv;vMist=step(.78,seed.w);
   vec2 anchor=floor(eye/24.0)*24.0+(seed.xy-.5)*720.0;
   // Emission stays attached to a world cell through camera movement.
   anchor=floor(anchor/24.0)*24.0;
   float random=hash(anchor),life=mix(2.4,9.0,vMist);
   float cycle=time/life+random*19.0,age=fract(cycle),birth=time-age*life;
   vec3 offset,normal;float crest;
   sea(anchor,birth,offset,normal,crest);
   vec3 world=vec3(anchor.x,seaLevel,anchor.y)+offset;
   world.xz+=wind*age*mix(14.0,32.0,vMist);
   world.y+=mix(1.0+age*9.0-age*age*5.0,3.0+age*2.0,vMist);
   float fade=smoothstep(0.0,.13,age)*(1.0-smoothstep(.35,1.0,age));
   float edge=1.0-smoothstep(260.0,345.0,length(anchor-eye));
   vAlpha=fade*edge*wet(world.xz)*mix(smoothstep(.27,.46,crest)*.46,.09,vMist)*stormStrength;
   #ifndef STORM
     vAlpha*=1.0-vMist; // wind-blown mist belongs to the storm
   #endif
   vec4 view=viewMatrix*vec4(world,1);
   vec2 size=mix(vec2(3.0+age*10.0,2.0+age*7.0),vec2(35.0,7.0),vMist);
   view.xy+=position.xy*size;
   vDepth=-view.z;gl_Position=projectionMatrix*view;
 }
