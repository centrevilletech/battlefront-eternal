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
#define SHADER_NAME destruction-smoke
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


  attribute vec4 a0; // birth position, birth time
  attribute vec4 a1; // velocity, lifetime
  attribute vec4 a2; // size at birth, size at death, pull (g, + falls), drag
  attribute vec4 a3; // style, seed, opacity, floor height (chips, scorch)
  attribute vec4 a4; // tone (0 black .. 1 pale), heat, wind share, spin
  uniform float fxTime;
  uniform vec3 wind;
  uniform vec3 sunDirection;
  varying vec2 vUv;
  varying vec4 vState; // age 0..1, seed, opacity, style
  varying vec4 vLook;  // tone, heat, seconds, spin phase
  varying vec3 vSun;   // toward the sun, in view space
  varying float vDepth;
  
  vec3 dMove(vec3 p0, vec3 v0, vec3 pull, float k, float t, out vec3 v) {
    k = max(k, 0.01);
    float e = exp(-k * t);
    vec3 terminal = pull / k;
    v = (v0 - terminal) * e + terminal;
    return p0 + (v0 - terminal) * (1.0 - e) / k + terminal * t;
  }

  void main() {
    float t = max(fxTime - a0.w, 0.0);
    float age = clamp(t / max(a1.w, 0.001), 0.0, 1.0);
    float style = a3.x;
    vec3 v;
    vec3 p = dMove(a0.xyz, a1.xyz, vec3(0.0, -9.8 * a2.z, 0.0), a2.w, t, v);
    float size = a2.x;
    vec2 corner = position.xy;
    vec4 view;
    if (style > 1.5) {
      // A scorch mark, flat on the floor.
      float c = cos(a3.y * 6.2832), s = sin(a3.y * 6.2832);
      vec2 spread = vec2(c * corner.x - s * corner.y, s * corner.x + c * corner.y) * a2.x;
      view = viewMatrix * vec4(p.x + spread.x, a3.w + 0.03, p.z + spread.y, 1.0);
    } else {
      if (style < 0.5) {
        // Smoke: carried off by the wind as it rises, curling a little.
        p += wind * a4.z * t * smoothstep(0.0, 1.5, t);
        p.x += sin(a3.y * 11.0 + t * 0.7) * 0.15 * t;
        p.z += cos(a3.y * 7.0 + t * 0.6) * 0.15 * t;
        size = mix(a2.x, a2.y, 1.0 - pow(1.0 - age, 2.4));
      } else if (p.y < a3.w) {
        p.y = a3.w + (a3.w - p.y) * 0.25;
      }
      view = viewMatrix * vec4(p, 1.0);
      float angle = a3.y * 6.2832 + a4.w * t;
      float c = cos(angle), s = sin(angle);
      if (style > 0.5) corner.y *= 0.55 + 0.45 * abs(sin(a3.y * 9.0 + a4.w * t * 0.6));
      vec2 offset = vec2(c * corner.x - s * corner.y, s * corner.x + c * corner.y) * size;
      if (style < 0.5) {
        // Drawn out along its motion on screen: a trail or a rising wisp
        // reads as a streak, a puff come to rest stays round.
        vec2 sv = (viewMatrix * vec4(v + wind * a4.z, 0.0)).xy;
        float speed = length(sv);
        vec2 along = speed > 1e-3 ? sv / speed : vec2(0.0, 1.0);
        vec2 across = vec2(-along.y, along.x);
        offset = along * dot(offset, along) * (1.0 + min(speed * 0.45, 1.0)) + across * dot(offset, across);
      }
      view.xy += offset;
    }
    vUv = uv;
    vState = vec4(age, a3.y, a3.z, style);
    vLook = vec4(a4.x, a4.y, t, a4.w * t);
    vSun = normalize((viewMatrix * vec4(-sunDirection, 0.0)).xyz);
    vDepth = -view.z;
    gl_Position = projectionMatrix * view;
  }
