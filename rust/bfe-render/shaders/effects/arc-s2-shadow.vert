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
#define SHADER_NAME arc-lightning
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
  attribute vec3 iA;
  attribute vec3 iB;
  attribute vec3 iNA;   // the joint's plane at A (zero: A is an end)
  attribute vec3 iNB;   // the joint's plane at B
  attribute vec4 iColor; // rgb sheath colour (max channel 1), a brightness
  attribute vec4 iSize;  // core radius, glow radius, white-hot share
  uniform float pixelAngle; // image-plane units per pixel at unit depth
  uniform float nearDepth;  // just past the camera's near plane
  uniform float closeRange; // m from the eye where the glow starts to narrow (0: never)
  varying vec2 vPlane;
  varying vec3 vA;
  varying vec3 vB;
  varying vec3 vNA;
  varying vec3 vNB;
  varying vec4 vColor;
  varying vec3 vSize;
  varying float vEnergy;
  varying float vCoreShare;
  void main() {
    vec3 a = (modelViewMatrix * vec4(iA, 1.0)).xyz;
    vec3 b = (modelViewMatrix * vec4(iB, 1.0)).xyz;
    const float NEAR = 0.05;
    if (a.z > -NEAR && b.z > -NEAR) { gl_Position = vec4(2.0, 2.0, 2.0, 1.0); return; }
    bool clipA = a.z > -NEAR, clipB = b.z > -NEAR;
    if (clipA) a = mix(b, a, (b.z + NEAR) / (b.z - a.z));
    if (clipB) b = mix(a, b, (a.z + NEAR) / (a.z - b.z));
    float nearest = min(-a.z, -b.z);
    // Close to the eye (a shooter's own bolt in first person) the glow
    // narrows, so the gun's end doesn't fill the view.
    float close = closeRange > 0.0 ? clamp(nearest / closeRange, 0.3, 1.0) : 1.0;
    // Held to a couple of pixels far away, its brightness thinned to match,
    // so a distant bolt stays visible without growing brighter.
    float glow = max(iSize.y * close, nearest * pixelAngle * 2.2);
    float core = max(iSize.x * close, nearest * pixelAngle * 0.65);
    vEnergy = clamp(iSize.y * close / glow, 0.12, 1.0);
    vCoreShare = clamp(iSize.x * close / core, 0.0, 1.0);
    vec2 ap = a.xy / -a.z, bp = b.xy / -b.z;
    float reach = glow * 2.4;
    float ra = reach / -a.z, rb = reach / -b.z, r = max(ra, rb);
    vec2 dir = bp - ap;
    float len = length(dir);
    dir = len > 1e-7 ? dir / len : vec2(1.0, 0.0);
    vec2 across = vec2(-dir.y, dir.x);
    float along = corner.x * 0.5 + 0.5;
    vec2 p = mix(ap - dir * ra, bp + dir * rb, along) + across * r * corner.y;
    vPlane = p;
    vA = a;
    vB = b;
    mat3 toView = mat3(modelViewMatrix);
    vNA = clipA ? vec3(0.0) : toView * iNA;
    vNB = clipB ? vec3(0.0) : toView * iNB;
    vColor = iColor;
    vSize = vec3(core, glow, iSize.z);
    // All four corners share one depth (past the near plane, so a segment
    // at the gun in first person still draws), so the image-plane position
    // interpolates linearly across the quad.
    float depth = max(nearest, nearDepth);
    gl_Position = projectionMatrix * vec4(p * depth, -depth, 1.0);
  }
