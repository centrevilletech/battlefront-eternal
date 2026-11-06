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


  attribute vec2 corner;
  attribute vec3 iHead;
  attribute vec3 iTail;
  attribute vec4 iColor; // rgb (max channel 1), brightness
  attribute vec4 iSize;  // core radius, glow radius, kind (0 tracer line, 1 vapour, 2 bolt or spark), seed
  attribute vec4 iMisc;  // age (s), vapour opacity (a bolt's: metres it fades in over), metres from the muzzle at the tail, 1 if it may sit before the eye
  uniform float pixelAngle;
  varying vec2 vPlane;
  varying vec3 vHead;
  varying vec3 vTail;
  varying vec4 vColor;
  varying vec4 vSize;
  varying vec4 vMisc;
  varying float vEnergy;
  varying float vCoreShare;
  void main() {
    vec3 h = (modelViewMatrix * vec4(iHead, 1.0)).xyz;
    vec3 t = (modelViewMatrix * vec4(iTail, 1.0)).xyz;
    const float NEAR = 0.06;
    if (h.z > -NEAR && t.z > -NEAR) { gl_Position = vec4(2.0, 2.0, 2.0, 1.0); return; }
    vec3 h0 = h, t0 = t;
    if (h.z > -NEAR) h = mix(t0, h0, (t0.z + NEAR) / (t0.z - h0.z));
    if (t.z > -NEAR) t = mix(h0, t0, (h0.z + NEAR) / (h0.z - t0.z));
    float nearest = min(-h.z, -t.z);
    bool vapor = iSize.z > 0.5 && iSize.z < 1.5;
    float glow = vapor ? iSize.y : max(iSize.y, nearest * pixelAngle * 2.5);
    float core = max(iSize.x, nearest * pixelAngle * 0.75);
    vEnergy = vapor ? 1.0 : clamp(iSize.y / glow, 0.08, 1.0);
    vCoreShare = clamp(iSize.x / core, 0.0, 1.0);
    vec2 hp = h.xy / -h.z, tp = t.xy / -t.z;
    // The quad reaches only as far from the segment as the fragment shader
    // can light a pixel: beyond it every pixel is let go (its discards),
    // so the quad can stop there. Each kind's bound is its brightest term
    // solved for the 0.002 cut, with the shader's own peak factors folded
    // in (vapour: body 1.3 x keep x (0.9 a + 0.6 opacity); tracer line:
    // the halo 0.32 e^-2x, x 1.7 redGlow x 1.35 pulse, the sheath folded in
    // as 1.5 % past x = 2; bolt and spark: the sheath 2 e^-2.4x^2 with its
    // red glow 1.25 wider and 1.7 brighter).
    float energy = iColor.a * vEnergy;
    float reach;
    if (vapor) {
      float radius = glow * 0.5;
      reach = radius * sqrt(max(-log(0.0019 / (1.3 * (0.9 * iColor.a + 0.6 * iMisc.y))), 0.0) / 1.6);
    } else if (iSize.z < 0.5) {
      reach = glow * max(2.0, 0.5 * log(392.0 * energy));
    } else {
      reach = glow * sqrt(max(log(1790.0 * energy), 0.0) * 0.651);
    }
    // A pixel's distance from the segment falls short of its offset in the
    // plane (times the depth) by its ray's slant, 1 / |(p, 1)|: each end's
    // half width is widened to what covers the slant at the quad's own far
    // edge (r = k sqrt(1 + (|p| + r)^2), k the reach over the depth).
    float kh = min(reach / -h.z, 0.9), kt = min(reach / -t.z, 0.9);
    float lh = length(hp), lt = length(tp);
    float rh = (kh * kh * lh + kh * sqrt(max(1.0 + lh * lh - kh * kh, 0.0))) / (1.0 - kh * kh);
    float rt = (kt * kt * lt + kt * sqrt(max(1.0 + lt * lt - kt * kt, 0.0))) / (1.0 - kt * kt);
    float r = max(rh, rt);
    vec2 dir = tp - hp;
    float len = length(dir);
    dir = len > 1e-7 ? dir / len : vec2(1.0, 0.0);
    vec2 across = vec2(-dir.y, dir.x);
    float along = corner.x * 0.5 + 0.5;
    vec2 p = mix(hp - dir * rh, tp + dir * rt, along) + across * r * corner.y;
    vPlane = p;
    vHead = h;
    vTail = t;
    vColor = iColor;
    vSize = vec4(core, glow, iSize.z, iSize.w);
    // The tail's distance from the muzzle, carried to where the clipped
    // tail now starts, so the noise stays fixed along the path.
    float cut = length(t - t0);
    vMisc = vec4(iMisc.x, iMisc.y, iMisc.z + cut, iMisc.w);
    float depth = nearest;
    gl_Position = projectionMatrix * vec4(p * depth, -depth, 1.0);
  }
