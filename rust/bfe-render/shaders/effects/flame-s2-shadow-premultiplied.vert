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


  attribute vec3 nozzle, direction, bend;
  attribute float nozzleRadius, phase;
  uniform float flameTime, power;
  varying vec3 boxPoint, rayOrigin, axisRight, axisUp, axisForward, origin;
  varying vec2 extent, curvature;
  varying float radius, lengthJet, seed;
  void main() {
    vec3 forward = normalize(direction);
    vec3 right = cross(forward, vec3(0.0,1.0,0.0));
    if (dot(right,right) < 0.01) right = cross(forward,vec3(0.0,0.0,1.0));
    right = normalize(right);
    vec3 up = cross(forward,right);
    curvature = vec2(dot(bend,right),dot(bend,up));
    radius = nozzleRadius;
    lengthJet = (0.61 + 0.025 * sin(flameTime * 19.0 + phase)) * power;
    extent = vec2(radius * 3.8) + abs(curvature);
    // Proxy extends just inside the nozzle lip. Density itself starts at z=0.
    boxPoint = vec3(position.xy * 2.0, position.z + 0.5);
    vec3 eye = cameraPosition - nozzle;
    rayOrigin = vec3(dot(eye,right)/extent.x,dot(eye,up)/extent.y,dot(eye,forward)/lengthJet);
    vec3 world = nozzle + right * boxPoint.x * extent.x + up * boxPoint.y * extent.y + forward * boxPoint.z * lengthJet;
    axisRight=right;axisUp=up;axisForward=forward;origin=nozzle;seed=phase;
    gl_Position = projectionMatrix * viewMatrix * vec4(world,1.0);
  }
