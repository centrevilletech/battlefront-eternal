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


      attribute vec4 seed;uniform float sprayTime;uniform vec4 sources[5],wakes[8],motion[8];
      varying vec2 vUv;varying float alpha,depth;
      void main(){
        float angle=seed.y*6.2831853;int i=int(seed.x);
        vec3 origin=wakes[i].xyz+vec3(0.,.15,0.);
        float age=sprayTime-wakes[i].w,life=.7,speed=(.7+seed.w*2.)*motion[i].w;
        vec3 velocity=vec3(cos(angle)*speed,2.+seed.z*2.,sin(angle)*speed);
        float size=.028+seed.w*.035;alpha=age<0.||age>life?0.:1.;
        // The water around the fountain's nozzle: Enhanced fills the inner
        // tier to its lip (TIER, fountain-jets.js), 4 m out, above the outer pool.
        bool wet=false;for(int j=0;j<5;j++)if(sources[j].w>.5&&distance(origin.xz,sources[j].xz)<6.3&&abs(origin.y-sources[j].y)<1.8){wet=true;origin.y=sources[j].y+(distance(origin.xz,sources[j].xz)<3.9?.58:-.25);}
        if(!wet)alpha=0.;
        vec3 world=origin+velocity*age+vec3(0.,-7.5*age*age,0.);
        vec4 eye=viewMatrix*vec4(world,1.);
        eye.xy+=position.xy*vec2(size,size*(1.+abs(velocity.y-15.*age)*.12));
        depth=-eye.z;vUv=uv;alpha*=smoothstep(0.,.10,age)*(1.-smoothstep(life*.8,life,age));
        if(alpha<=0.)eye=vec4(0.,0.,-1e6,1.);
        gl_Position=projectionMatrix*eye;
      }