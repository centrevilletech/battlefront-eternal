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
#define FLIP_SIDED
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


attribute vec3 origin, direction, extent;
attribute float kind;
varying vec3 point, eye, base, rightAxis, upAxis, forwardAxis, size;
varying float type;
void main() {
  vec3 up=normalize(direction),right=cross(up,vec3(0.,0.,1.));
  if(dot(right,right)<.01)right=cross(up,vec3(1.,0.,0.));
  right=normalize(right);vec3 forward=cross(right,up);
  point=position+vec3(0.,.5,0.);size=extent;base=origin;type=kind;
  rightAxis=right;upAxis=up;forwardAxis=forward;
  vec3 relative=cameraPosition-base;
  eye=vec3(dot(relative,right),dot(relative,up),dot(relative,forward))/size;
  vec3 world=base+right*point.x*size.x+up*point.y*size.y+forward*point.z*size.z;
  gl_Position=projectionMatrix*viewMatrix*vec4(world,1.);
}