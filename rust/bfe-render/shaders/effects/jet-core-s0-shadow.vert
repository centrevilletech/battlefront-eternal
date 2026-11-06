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


  attribute vec3 sweep; // part (0 the jet, 1 the dome), along it from the bottom, around
  uniform vec4 sources[5];
  uniform float levels[5];
  uniform float jetTime;
  uniform vec3 sunDirection;
  varying vec3 vWorld, vNormal, vFlow;
  varying float vY, vAbove, vAlpha, vLit, vPart, vDepth;
  const float G = 9.81, APEX = 8.00, TOP = 6.40, TAU = 6.2831853;
  
  uniform float visible[5];
  int fountain(int block) { return int(visible[gl_InstanceID / block] + 0.5); }

  
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
    int i = fountain(1);
    vec4 origin = sources[i];
    float launch = sqrt(2.0 * G * APEX), turn = sweep.z * TAU, level = levels[i] - origin.y;
    vec2 around = vec2(cos(turn), sin(turn));
    vec3 local, normal;
    if (sweep.x < 0.5) {
      // From the pool's surface up.
      float y = mix(level, TOP, sweep.y);
      float radius = 0.1 + 0.06 * y + 0.01 * sin(turn * 3.0 + y * 1.7);
      local = vec3(around.x * radius, y, around.y * radius);
      normal = vec3(around.x, -0.05, around.y);
      vFlow = vec3(around, jetTime - (launch - sqrt(max(launch * launch - 2.0 * G * y, 0.0))) / G);
      vY = y;
    } else {
      // Heaving up around the jet's foot and spreading over the pool.
      float t = sweep.y, heave = 0.05 * sin(jetTime * 5.3 + turn * 3.0) + 0.04 * sin(jetTime * 7.9 - turn * 5.0 + t * 4.0);
      float radius = mix(0.95, 0.12, t);
      local = vec3(around.x * radius, level + (0.18 + heave) * sin(t * 1.5708), around.y * radius);
      normal = vec3(around.x * cos(t * 1.5708), 1.0, around.y * cos(t * 1.5708));
      vFlow = vec3(local.xz * 3.0, jetTime * 1.6);
      vY = t;
    }
    vec3 world = origin.xyz + local;
    vWorld = world;
    vNormal = normalize(normal);
    vAbove = local.y - level;
    vAlpha = origin.w;
    vLit = 0.25 + 0.75 * sunThrough(local);
    vPart = sweep.x;
    vec4 view = viewMatrix * vec4(world, 1.0);
    vDepth = -view.z;
    gl_Position = projectionMatrix * view;
  }
