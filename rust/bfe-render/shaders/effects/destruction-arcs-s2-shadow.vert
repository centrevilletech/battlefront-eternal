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
#define SHADER_NAME destruction-arcs
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


  attribute vec3 tangent;
  attribute vec4 meta; // side (-1, 1), along 0..1, half width (m), intensity
  uniform vec2 resolution;
  varying vec4 vMeta;
  varying float vDepth;
  void main() {
    vec4 view = viewMatrix * vec4(position, 1.0);
    vec3 along = (viewMatrix * vec4(tangent, 0.0)).xyz;
    vec3 side = cross(along, normalize(view.xyz));
    float length2 = dot(side, side);
    side = length2 > 1e-10 ? side * inversesqrt(length2) : vec3(1.0, 0.0, 0.0);
    // Never thinner than a couple of pixels, so a far arc still crackles.
    float pixel = 2.0 / (projectionMatrix[1][1] * max(resolution.y, 1.0));
    float width = max(meta.z, -view.z * pixel * 2.2);
    view.xyz += side * meta.x * width;
    vMeta = meta;
    vDepth = -view.z;
    gl_Position = projectionMatrix * view;
  }
