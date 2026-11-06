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
#define SHADER_NAME destruction-hot
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
  attribute vec4 a3; // style, seed, intensity, floor height (sparks, glow) or tint (0 fire, 1 electric)
  uniform float fxTime;
  varying vec2 vUv;
  varying vec4 vState; // age 0..1, seed, intensity, style
  varying float vTint;
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
    if (style < 0.5 && p.y < a3.w) {
      // A spark that reaches the floor skips along it, slowed.
      p.y = a3.w + (a3.w - p.y) * 0.3;
      v.y = -v.y * 0.3;
      v.xz *= 0.55;
    }
    if (style > 3.5) {
      // Embers wander as they rise.
      p.x += sin(a3.y * 7.1 + t * 2.3) * 0.12 * t;
      p.z += cos(a3.y * 5.3 + t * 1.9) * 0.12 * t;
    }
    float size = mix(a2.x, a2.y, 1.0 - (1.0 - age) * (1.0 - age));
    vec2 corner = position.xy;
    vec4 view;
    if (style > 2.5 && style < 3.5) {
      // Glow on the floor: flat on it, under the blast.
      float c = cos(a3.y * 6.2832), s = sin(a3.y * 6.2832);
      vec2 spread = vec2(c * corner.x - s * corner.y, s * corner.x + c * corner.y) * size;
      view = viewMatrix * vec4(p.x + spread.x, a3.w + 0.04, p.z + spread.y, 1.0);
    } else {
      view = viewMatrix * vec4(p, 1.0);
      if (style < 0.5) {
        // A streak behind the spark, as long as its motion over ~1/25 s.
        vec3 sv = (viewMatrix * vec4(v, 0.0)).xyz;
        float speed = length(sv.xy);
        vec2 along = speed > 1e-4 ? sv.xy / speed : vec2(0.0, 1.0);
        vec2 across = vec2(-along.y, along.x);
        float reach = clamp(length(v) * 0.03, size * 2.0, 0.45);
        view.xy += across * corner.x * size + along * (corner.y * 0.5 - 0.5) * reach;
      } else {
        float angle = a3.y * 6.2832 + t * (style < 1.5 ? 1.2 : 0.0) * (fract(a3.y * 13.7) - 0.5);
        float c = cos(angle), s = sin(angle);
        view.xy += vec2(c * corner.x - s * corner.y, s * corner.x + c * corner.y) * size;
      }
    }
    vUv = uv;
    vState = vec4(age, a3.y, a3.z, style);
    vTint = a3.w;
    vDepth = -view.z;
    gl_Position = projectionMatrix * view;
  }
