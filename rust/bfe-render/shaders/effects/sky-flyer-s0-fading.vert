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
#define SHADER_NAME sky-flyer
#define USE_INSTANCING
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


  uniform vec3 sunDirection;
  uniform vec3 sunColor;
  uniform vec3 ambient;
  uniform float reach;
  // fade, haze, past the far plane (1) or not, the scale it is drawn at
  attribute vec4 look;
  varying vec2 vUv;
  varying vec3 vLight;
  varying float vFade;
  varying float vHaze;
  varying vec3 vDirection;
  void main() {
    mat4 placed = modelMatrix;
    #ifdef USE_INSTANCING
      placed = placed * instanceMatrix;
    #endif
    #ifdef LIT
      // A prop's light: the ambient and the sun, per vertex.
      vec3 n = normalize(mat3(placed) * normal);
      vLight = clamp(ambient + max(dot(n, -sunDirection), 0.0) * sunColor, 0.0, 1.0);
    #else
      vLight = vec3(1.0);
    #endif
    vUv = uv;
    vFade = look.x;
    vHaze = look.y;
    vec4 world = placed * vec4(position, 1.0);
    vDirection = world.xyz - cameraPosition;
    gl_Position = projectionMatrix * viewMatrix * world;
    if (look.z > 0.5) {
      // Drawn scaled toward the eye (Enhanced): its depth goes just inside
      // the far plane, in the order of its true distance, behind everything
      // else the far scene draws.
      float far = length(vDirection) / max(look.w, 1e-6);
      float near = clamp(reach / far, 0.0, 1.0);
      gl_Position.z = gl_Position.w * (0.999999 - 9e-6 * near);
    }
  }
