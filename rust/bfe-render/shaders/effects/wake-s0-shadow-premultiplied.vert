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


  attribute vec4 birth; // position, exact emission time
  attribute vec4 flow;  // initial world velocity, lifetime
  attribute vec4 axis;  // exhaust axis, initial radius
  attribute vec2 style; // random phase, opacity
  uniform float wakeTime;
  uniform float kind;
  uniform float drag;
  varying vec2 wakeUv;
  varying vec4 wakeState;
  varying float viewDepth;
  void main() {
    float seconds = max(0.0, wakeTime - birth.w);
    float age = clamp(seconds / flow.w, 0.0, 1.0);
    vec3 direction = normalize(axis.xyz);
    vec3 tangent = cross(direction, abs(direction.y) > 0.9 ? vec3(1,0,0) : vec3(0,1,0));
    tangent = normalize(tangent);
    vec3 bitangent = cross(direction, tangent);
    float seed = style.x;
    float curl = axis.w * (kind > 0.5 && kind < 1.5 ? 1.8 : 0.7) * age;
    float phase = seed + seconds * (3.0 + fract(seed) * 4.0);
    vec3 vortex = tangent * (sin(phase) - sin(seed)) + bitangent * (cos(phase * 1.31) - cos(seed * 1.31));
    vec3 at = birth.xyz + flow.xyz * ((1.0 - exp(-drag * seconds)) / drag) + vortex * curl;
    at.y += seconds * seconds * (kind > 1.5 ? -1.0 : 0.35);
    if (kind > 0.5 && kind < 1.5) at.xz += vec2(0.10, -0.07) * seconds * seconds;
    vec4 view = viewMatrix * vec4(at, 1.0);
    vec2 motion = (viewMatrix * vec4(flow.xyz, 0.0)).xy;
    vec2 along = normalize(motion + vec2(0.0001));
    vec2 across = vec2(along.y, -along.x);
    float radius = axis.w * (kind > 1.5 ? 1.0 : mix(0.65, kind > 0.5 ? 2.0 : 1.5, age));
    float stretch = kind > 1.5 ? 3.8 : (kind > 0.5 ? 1.35 : 2.2);
    // Tilt each wisp around the velocity axis so the wake does not become
    // a stack of identically oriented circles or a single continuous sheet.
    float twist = kind > 1.5 ? 0.0 : sin(seed + age * 2.0) * 0.65;
    vec2 p = mat2(cos(twist), -sin(twist), sin(twist), cos(twist)) * position.xy;
    view.xy += across * p.x * radius + along * p.y * radius * stretch;
    wakeUv = uv;
    wakeState = vec4(age, seed, style.y, seconds);
    viewDepth = -view.z;
    gl_Position = projectionMatrix * view;
  }
