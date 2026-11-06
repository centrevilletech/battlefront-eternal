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
  attribute vec3 iHead;
  attribute vec3 iTail;
  attribute vec4 iColor; // rgb sheath colour (max channel 1), a brightness
  attribute vec4 iSize;  // core radius, glow radius, seed, tail taper
  uniform float pixelAngle; // image-plane units per pixel at unit depth
  varying vec2 vPlane;
  varying vec3 vHead;
  varying vec3 vTail;
  varying vec4 vColor;
  varying vec4 vSize;
  varying float vEnergy;
  varying float vCoreShare;
  void main() {
    vec3 h = (modelViewMatrix * vec4(iHead, 1.0)).xyz;
    vec3 t = (modelViewMatrix * vec4(iTail, 1.0)).xyz;
    const float NEAR = 0.06;
    if (h.z > -NEAR && t.z > -NEAR) { gl_Position = vec4(2.0, 2.0, 2.0, 1.0); return; }
    if (h.z > -NEAR) h = mix(t, h, (t.z + NEAR) / (t.z - h.z));
    if (t.z > -NEAR) t = mix(h, t, (h.z + NEAR) / (h.z - t.z));
    // Hold the halo to a couple of pixels far away and thin its brightness to
    // match, so a distant shot stays visible without growing brighter.
    float nearest = min(-h.z, -t.z);
    float glow = max(iSize.y, nearest * pixelAngle * 2.5);
    float core = max(iSize.x, nearest * pixelAngle * 0.75);
    vEnergy = clamp(iSize.y / glow, 0.08, 1.0);
    vCoreShare = clamp(iSize.x / core, 0.0, 1.0);
    vec2 hp = h.xy / -h.z, tp = t.xy / -t.z;
    float reach = glow * 3.2;
    float rh = reach / -h.z, rt = reach / -t.z, r = max(rh, rt);
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
    // All four corners share one depth, so the image-plane position
    // interpolates linearly across the quad.
    float depth = nearest;
    gl_Position = projectionMatrix * vec4(p * depth, -depth, 1.0);
  }
