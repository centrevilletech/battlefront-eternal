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


  attribute vec4 shore;
  attribute float phase;
  uniform float time;
  uniform float height;
  uniform float splash;
  varying vec2 vUv;
  varying float vPhase;
  varying float vPulse;
  varying vec3 vWorld;
  void main() {
    vUv = uv;
    vPhase = phase;
    vec2 normal = shore.zw;
    vec2 tangent = vec2(-normal.y, normal.x);
    float lap = 0.5 + 0.5 * sin(time * 1.35 + phase * 6.283);
    float event = fract(time * 0.115 + phase);
    vPulse = sin(clamp(event / 0.16, 0.0, 1.0) * 3.14159);
    float width = mix(0.28, 0.64, lap);
    vec2 at = shore.xy + tangent * (uv.x - 0.5) * 1.85;
    at += normal * (splash > 0.5 ? 0.06 + uv.y * 0.09 : 0.03 + uv.y * width);
    float y = height + 0.024 + (splash > 0.5 ? uv.y * (0.08 + 0.27 * vPulse) : 0.0);
    vec4 world = modelMatrix * vec4(at.x, y, at.y, 1.0);
    vWorld = world.xyz;
    gl_Position = projectionMatrix * viewMatrix * world;
  }
