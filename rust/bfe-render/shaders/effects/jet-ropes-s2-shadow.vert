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


  attribute vec2 strip; // along it from the crown, across it
  attribute float rope; // its number
  uniform vec4 sources[5];
  uniform float levels[5], lows[5]; // the tier's pool and the outer pool
  uniform float jetTime, side;
  uniform vec3 wind;
  uniform vec3 sunDirection;
  uniform vec2 depthResolution;
  varying vec3 vWorld, vSide, vAlong, vFlow;
  varying vec4 vRope;
  varying float vAcross, vHalf, vSpeed, vFall, vLit, vCover, vDepth, vLevel, vPixel;
  const float G = 9.81, APEX = 8.00, TAU = 6.2831853, ROPES = 170.0, STRANDS = 160.0;
  const float ARCS = 28.0, BUNDLES = 7.0, LIFE = 1.45;
  uniform float perFountain;
  
  uniform float visible[5];
  int fountain(int block) { return int(visible[gl_InstanceID / block] + 0.5); }

  
  float hash1(float p) { p = fract(p * 0.1031); p *= p + 33.33; p *= p + p; return fract(p); }
  float noise1(float x) { float i = floor(x), f = fract(x); f = f * f * (3.0 - 2.0 * f); return mix(hash1(i), hash1(i + 1.0), f); }
  vec4 hash4(float p) {
    vec4 q = fract(vec4(p) * vec4(.1031, .1030, .0973, .1099));
    q += dot(q, q.wzxy + 33.33);
    return fract((q.xxyz + q.yzzw) * q.zywx);
  }

  
  void ropeStart(float j, float salt, float born, out vec2 dir, out vec4 start) {
    float n = j * 1.913 + salt;
    vec4 h = hash4(j * 3.17 + salt * 11.0);
    vec4 w = 0.5 + 0.5 * sin(born * vec4(0.43, 0.53, 0.71, 0.61) + n * vec4(0.9, 1.7, 3.1, 2.3));
    if (j >= ROPES + STRANDS) {
      float theta = (j - ROPES - STRANDS + 0.5 + 0.6 * (h.x - 0.5)) / ARCS * TAU + salt + 0.08 * sin(born * 0.5 + n);
      dir = vec2(cos(theta), sin(theta));
      start = vec4(0.6 + 0.4 * w.x, 3.2 + 0.6 * w.y, APEX * (0.84 + 0.08 * w.z), 0.6 + 0.8 * w.w);
      return;
    }
    float fine = step(ROPES, j), bundle = floor(h.x * BUNDLES), reach = 0.85 + 0.3 * hash1(bundle * 7.1 + salt);
    float theta = h.y < 0.8 ? (bundle + hash1(bundle * 5.3 + salt)) / BUNDLES * TAU + (h.z - 0.5) * 0.45 : h.z * TAU;
    theta += salt + 0.2 * sin(born * 0.37 + n * 2.1) + 0.1 * sin(born * 0.91 + n * 5.3);
    dir = vec2(cos(theta), sin(theta));
    float close = step(h.w, 0.4) * (1.0 - fine);
    start = fine > 0.5 ? vec4(0.35 + 0.8 * w.x, (0.6 + 1.2 * w.y) * reach, APEX * (0.76 + 0.18 * w.z), 0.8 * w.w)
      : vec4(0.3 + 0.7 * w.x, mix((1.0 + 0.9 * w.y) * reach, 0.25 + 0.4 * w.y, close), APEX * (0.84 + 0.12 * w.z), 0.3 + 0.6 * w.w);
  }
  vec3 ropeAt(vec2 dir, vec4 start, float age) {
    float d = start.x + start.y * age;
    return vec3(dir.x * d, start.z + start.w * age - 0.5 * G * age * age, dir.y * d) + wind * 0.3 * age * age;
  }
  // Which pool a stream falls into: the tier's (inner) or the outer one.
  bool ropeOuter(float j) { return j >= ROPES + STRANDS; }
  // Where a stream's water that left the crown at born meets its pool.
  vec3 ropeLanding(float j, float salt, float born, float level, out vec2 dir) {
    vec4 start;
    ropeStart(j, salt, born, dir, start);
    return ropeAt(dir, start, (start.w + sqrt(start.w * start.w + 2.0 * G * max(start.z - level, 0.0))) / G);
  }
  // Its own look: radius (m), opacity, where it breaks into drops (as a
  // share of its fall), how aerated.
  vec4 ropeLook(float j, float salt) {
    vec4 h = hash4(j * 5.11 + salt * 7.3 + 3.0);
    if (j >= ROPES + STRANDS) return vec4(0.012 + 0.012 * h.x, 0.7 + 0.3 * h.y, 0.55 + 0.35 * h.z, 0.45 + 0.4 * h.w);
    return j < ROPES ? vec4(0.011 * exp(1.5 * h.x), 0.6 + 0.4 * h.y, 0.3 + 0.6 * h.z, 0.4 + 0.6 * h.w)
      : vec4(0.004 + 0.008 * h.x, 0.5 + 0.4 * h.y, 0.15 + 0.45 * h.z, 0.25 + 0.45 * h.w);
  }

  
  float sunThrough(vec3 p) {
    vec3 L = -sunDirection;
    float reach = max(APEX + 0.4 - p.y, 0.0) / max(L.y, 0.2), depth = 0.0;
    for (int k = 0; k < 8; k++) {
      vec3 q = p + L * reach * (float(k) + 0.5) / 8.0;
      float r = length(q.xz), R = 0.6 + 1.45 * sqrt(max(2.0 * (APEX * 0.9 - q.y) / 9.81, 0.0));
      depth += 0.9 * exp(-pow((r - R) / 0.3, 2.0)) * step(q.y, APEX * 0.95) * reach / 8.0;
    }
    float l2 = max(dot(L.xz, L.xz), 1e-4), tc = -dot(p.xz, L.xz) / l2, rc = 0.15 + 0.05 * clamp(p.y + L.y * tc, 0.0, APEX);
    float miss = length(p.xz + L.xz * tc), half_ = sqrt(max(rc * rc - miss * miss, 0.0)) / sqrt(l2);
    if (p.y + L.y * tc < APEX * 0.8) depth += 2.5 * clamp(tc + half_, 0.0, 2.0 * half_);
    vec3 c = vec3(0.0, APEX * 0.88, 0.0) - p;
    float tb = dot(c, L), across = sqrt(max(1.0 - (dot(c, c) - tb * tb), 0.0));
    depth += 0.8 * clamp(tb + across, 0.0, 2.0 * across);
    return exp(-0.7 * depth);
  }

  void main() {
    int i = fountain(int(perFountain));
    vec4 source = sources[i];
    float j = rope, salt = float(i) * 2.17, pool = ropeOuter(j) ? lows[i] : levels[i], level = pool - source.y;
    // The whole ribbon in one half, by the way it falls now.
    vec2 dir; vec4 start;
    ropeStart(j, salt, jetTime, dir, start);
    if ((dot(dir, cameraPosition.xz - source.xz) >= 0.0 ? 1.0 : 0.0) != side) { gl_Position = vec4(2.0, 2.0, 2.0, 1.0); return; }
    float age = LIFE * pow(strip.x, 1.5), born = jetTime - age;
    ropeStart(j, salt, born, dir, start);
    vec3 p = ropeAt(dir, start, age);
    // Along the path: where water that left a moment earlier is now.
    vec2 dir1; vec4 start1;
    ropeStart(j, salt, born - 0.01, dir1, start1);
    vec3 along = ropeAt(dir1, start1, age + 0.01) - p;
    along = length(along) > 1e-6 ? normalize(along) : vec3(0.0, -1.0, 0.0);
    vec3 center = source.xyz + p, toEye = normalize(cameraPosition - center);
    vec3 across = cross(along, toEye);
    across = length(across) > 1e-5 ? normalize(across) : vec3(1.0, 0.0, 0.0);
    float depth = -(viewMatrix * vec4(center, 1.0)).z;
    float pixel = 2.0 * max(depth, 0.1) / (projectionMatrix[1][1] * depthResolution.y);
    vec4 look = ropeLook(j, salt);
    // Wide enough for the stream, its waver and its drops, and at least a pixel.
    float halfWidth = max(2.4 * look.x, pixel);
    vec3 world = center + across * strip.y * halfWidth;
    vWorld = world;
    vSide = across;
    vAlong = along;
    vFlow = vec3(born, j, salt);
    vRope = look;
    vAcross = strip.y;
    vHalf = halfWidth;
    vSpeed = length(vec3(dir.x * start.y, start.w - G * age, dir.y * start.y) + wind * 0.6 * age);
    vFall = clamp((start.z - p.y) / max(start.z - level, 1.0), 0.0, 1.0);
    vLit = 0.25 + 0.75 * sunThrough(world - source.xyz);
    vCover = smoothstep(0.0, 0.05, age);
    vPixel = pixel;
    vLevel = pool;
    vec4 view = viewMatrix * vec4(world, 1.0);
    vDepth = -view.z;
    gl_Position = projectionMatrix * view;
  }
