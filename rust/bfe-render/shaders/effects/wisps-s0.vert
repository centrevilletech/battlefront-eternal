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


      attribute vec4 seed;uniform float wispTime;uniform vec4 sources[9],directions[9];
      uniform vec4 wakes[8],motion[8];varying vec2 vUv;varying vec3 world;varying float alpha,depth,turn;
      void main(){
        int i=int(seed.x);vec3 up=normalize(directions[i].xyz+vec3(0.,1e-6,0.));
        vec3 right=cross(up,vec3(0.,0.,1.));if(dot(right,right)<.01)right=cross(up,vec3(1.,0.,0.));
        right=normalize(right);vec3 forward=cross(right,up);
        bool pipe=directions[i].w>.5;float life=pipe?.72:3.6,age=mod(wispTime+seed.y*life,life),t=age/life;
        float angle=seed.z*6.2831853;
        float r=pipe?.07:(.15+seed.w*.9);
        vec3 origin=sources[i].xyz+right*cos(angle)*r+forward*sin(angle)*r;
        world=origin+up*age*(pipe?4.2:1.25+seed.w*.45);
        world+=(right*sin(wispTime*.45+angle+t*2.)+forward*cos(wispTime*.37+angle+t*2.))*t*t*(pipe?.15:.75);
        for(int j=0;j<8;j++){
          float a=wispTime-wakes[j].w;if(a>=0.&&a<2.){
            vec3 delta=world-wakes[j].xyz;delta.y-=clamp(delta.y,.1,1.6);
            float force=exp(-dot(delta,delta)*1.5)*exp(-a*1.5)*motion[j].w;
            world+=motion[j].xyz*force*.12;
          }
        }
        float size=pipe?mix(.18,.85,t):mix(.65,2.2,t);
        size*=.8+seed.w*.35;
        vec3 cameraRight=vec3(viewMatrix[0][0],viewMatrix[1][0],viewMatrix[2][0]);
        vec3 cameraUp=vec3(viewMatrix[0][1],viewMatrix[1][1],viewMatrix[2][1]);
        world+=cameraRight*position.x*size+cameraUp*position.y*size;
        vec4 eye=viewMatrix*vec4(world,1.);depth=-eye.z;vUv=uv;turn=angle+t*.4;
        alpha=sources[i].w*smoothstep(0.,.12,t)*(1.-smoothstep(.35,1.,t))*(pipe?.16:.20);
        if(alpha<=0.)eye=vec4(0.,0.,-1e6,1.);gl_Position=projectionMatrix*eye;
      }