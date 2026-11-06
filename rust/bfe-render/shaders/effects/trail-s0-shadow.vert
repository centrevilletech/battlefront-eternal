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


  attribute vec4 birth; // world position, birth time
  attribute vec4 drift; // velocity, lifetime
  attribute vec4 shape; // initial size, seed, opacity, terrain dust
  uniform float trailTime;
  varying vec2 dustUv;
  varying vec4 dustShape;
  varying float age;
  varying float viewDepth;
  void main() {
    age = clamp((trailTime - birth.w) / drift.w, 0.0, 1.0);
    float seconds = max(trailTime - birth.w, 0.0);
    vec3 at = birth.xyz + drift.xyz * (1.0 - exp(-seconds * 1.8)) / 1.8;
    at.xz += vec2(sin(shape.y + seconds * 1.6), cos(shape.y * 1.7 + seconds)) * seconds * 0.13;
    at.y += seconds * 0.18;
    vec4 view = viewMatrix * vec4(at, 1.0);
    float size = shape.x * (0.45 + age * 2.8);
    float angle = shape.y + age * 0.35;
    vec2 p = position.xy;
    p = mat2(cos(angle), -sin(angle), sin(angle), cos(angle)) * p;
    view.xy += p * size * vec2(1.6, 0.85);
    viewDepth = -view.z;
    dustUv = uv;
    dustShape = shape;
    gl_Position = projectionMatrix * view;
  }
