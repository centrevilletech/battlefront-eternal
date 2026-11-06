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


  attribute vec4 seed;
  uniform vec3 eye, cameraMotion;
  uniform vec2 wind, depthResolution;
  uniform float rainTime, range, velocity, velocityRange, width, streak, enhanced, effect;
  uniform sampler2D cover;
  uniform float coverCell,coverSize;
  varying vec2 vUv;
  varying vec3 vWorld;
  varying float vOpacity,vDepth,vGlint;
  void main() {
    float radius = mix(range,range*mix(.22,1.0,seed.w*seed.w),enhanced);
    float span = radius * 2.0;
    float speed = max(0.1, velocity + (seed.w - 0.5) * velocityRange);
    speed *= mix(1.0,1.25+seed.z*.6,enhanced);
    vec2 gust = wind*(1.0+enhanced*2.0) + enhanced*vec2(sin(rainTime*.7)*4.5,cos(rainTime*.43)*2.0);
    vec2 drift = wind*rainTime*(1.0+enhanced*2.0)+enhanced*vec2(-cos(rainTime*.7)*6.43,sin(rainTime*.43)*4.65);
    vec3 center;
    center.xz = mod(seed.xy * span + drift - eye.xz + radius, span) + eye.xz - radius;
    center.y = eye.y + radius - mod(seed.z * span + rainTime * speed, span);
    vec3 fall = normalize(vec3(gust.x, -speed, gust.y)-cameraMotion*.2*enhanced);
    vec3 viewFall = normalize(mat3(viewMatrix) * fall);
    vec3 side = normalize(vec3(-viewFall.y, viewFall.x + 0.00001, 0.0));
    float length = streak*mix(1.0,mix(.18,.45+seed.z*.55,step(.18,seed.w)),enhanced);
    float thickness = width*mix(1.0,.35+seed.w*.65,enhanced);
    vOpacity = (1.0 - smoothstep(radius * 0.7, radius, distance(center.xz, eye.xz)))
      * smoothstep(0.35, 1.2, distance(center, eye));
    if(effect>.5){
      vec2 cell=mod(floor(center.xz/coverCell),coverSize);
      float floorY=texture2D(cover,(cell+.5)/coverSize).r;
      center.y=floorY+.4+seed.z;
      vOpacity*=step(floorY,eye.y+.5)*step(eye.y-10.0,floorY);
    }
    vec4 view = viewMatrix * vec4(center, 1.0);
    // Keep fine rain visible through MSAA/FXAA and the map's soft-focus pass.
    // Bound the minimum in pixels, so distant drops never become thick rods.
    if(enhanced>.5)thickness=max(thickness,.85*max(0.0,-view.z)/(projectionMatrix[1][1]*max(1.0,depthResolution.y)));
    if(effect>.5)view.xy+=position.xy*vec2(3.0,2.0);
    else view.xyz += side * position.x * thickness + viewFall * position.y * length;
    vWorld = center + (effect>.5?vec3(0,position.y*2.0,0):fall * position.y * length);
    vGlint=seed.w;
    vDepth=-view.z;vUv = uv;
    gl_Position = projectionMatrix * view;
  }
