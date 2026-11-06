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


  attribute vec2 corner;
  attribute vec3 iCenter;
  attribute vec3 iAxis;  // a ring's facing; zero for one that faces the camera
  attribute vec4 iColor; // rgb, brightness
  attribute vec4 iParam; // radius, ring width (of the radius), kind (0 flare, 1 ring), turn
  varying vec2 vCorner;
  varying vec4 vColor;
  varying vec4 vParam;
  void main() {
    vCorner = corner;
    vColor = iColor;
    vParam = iParam;
    float radius = iParam.x;
    vec4 view;
    if (dot(iAxis, iAxis) > 0.25) {
      vec3 a = normalize(iAxis);
      vec3 u = normalize(cross(a, abs(a.y) < 0.9 ? vec3(0.0, 1.0, 0.0) : vec3(1.0, 0.0, 0.0)));
      vec3 v = cross(a, u);
      view = modelViewMatrix * vec4(iCenter + (u * corner.x + v * corner.y) * radius, 1.0);
    } else {
      view = modelViewMatrix * vec4(iCenter, 1.0);
      // Pulled toward the eye, so a flash on a wall isn't cut in half by it.
      view.xyz += normalize(-view.xyz) * min(radius * 0.6, max(-view.z - 0.1, 0.0));
      float c = cos(iParam.w), s = sin(iParam.w);
      view.xy += mat2(c, s, -s, c) * corner * radius;
    }
    gl_Position = projectionMatrix * view;
  }
