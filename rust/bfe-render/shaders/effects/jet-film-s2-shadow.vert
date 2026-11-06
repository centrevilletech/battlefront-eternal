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


  attribute vec3 film; // side of the tier, along it, down its face from the lip
  uniform vec4 sources[5];
  uniform float levels[5], lows[5];
  uniform float jetTime, side;
  uniform vec3 sunDirection;
  varying vec3 vWorld, vNormal, vDown;
  varying vec2 vFlow;
  varying float vS, vLit, vDepth;
  const float G = 9.81, APEX = 8.00, TAU = 6.2831853, SIDES = 12.0;
  const float LIP = 4.00, FOOT = 4.82;
  
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
    float middle = (film.x + 0.5) * TAU / SIDES;
    vec2 out_ = vec2(cos(middle), sin(middle));
    if ((dot(out_, cameraPosition.xz - origin.xz) >= 0.0 ? 1.0 : 0.0) != side) { gl_Position = vec4(2.0, 2.0, 2.0, 1.0); return; }
    // The lip's stone lies just under the tier's pool; the face meets the
    // outer pool at its foot.
    float top = levels[i] - 0.01 - origin.y, bottom = lows[i] - origin.y;
    float a0 = film.x * TAU / SIDES, a1 = a0 + TAU / SIDES;
    vec2 along = mix(vec2(cos(a0), sin(a0)), vec2(cos(a1), sin(a1)), film.y) * mix(LIP, FOOT, film.z);
    // Out and up from the face, and down it.
    float run = (FOOT - LIP) * cos(3.14159265 / SIDES), drop = top - bottom;
    vec3 normal = normalize(vec3(out_ * drop, run).xzy), down = normalize(vec3(out_ * run, -drop).xzy);
    vec3 local = vec3(along.x, mix(top, bottom, film.z), along.y) + normal * 0.018;
    // The water leaves the lip slowly and speeds down the face (gravity
    // along the slope, less the stone's drag): when the water here left it.
    float slope = length(vec2(run, drop)), accel = G * drop / slope * 0.6, v0 = 0.5;
    float tau = (sqrt(v0 * v0 + 2.0 * accel * film.z * slope) - v0) / accel;
    vec3 world = origin.xyz + local;
    vWorld = world;
    vNormal = normal;
    vDown = down;
    vFlow = vec2((film.x + film.y) * LIP * TAU / SIDES, jetTime - tau);
    vS = film.z;
    vLit = 0.25 + 0.75 * sunThrough(local);
    vec4 view = viewMatrix * vec4(world, 1.0);
    vDepth = -view.z;
    gl_Position = projectionMatrix * view;
  }
