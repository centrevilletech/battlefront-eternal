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
  attribute vec3 spawnPosition, spawnVelocity;
  attribute vec4 spawnColor, spawnShape, spawnMeta;
  uniform sampler2D tracks, emitters;
  uniform float trackRows, emitterRows;
  vec4 track(float column) { return texture2D(tracks, vec2((column + 0.5) / 64.0, (spawnMeta.w + 0.5) / trackRows)); }
  vec4 emitter(float column) { return texture2D(emitters, vec2((column + 0.5) / 3.0, (spawnMeta.z + 0.5) / emitterRows)); }
  vec3 rotateBy(vec3 p, vec4 q) { return p + 2.0 * cross(q.xyz, cross(q.xyz, p) + q.w * p); }

  varying vec2 vUv;
  varying vec4 vColor;
  varying float vDepth;
  varying float vSoftDistance;
  void main() {
    
  vec4 origin = emitter(0.0), turn = emitter(1.0), clock = emitter(2.0);
  float age = max(0.0, clock.x - spawnMeta.x);
  vec4 counts = track(0.0), settings = track(1.0), atlas = track(2.0);
  vec3 velocity = spawnVelocity, displacement = vec3(0.0);
  float left = age;
  for (int k = 0; k < 8; k++) {
    if (float(k) >= counts.x || left <= 0.0) break;
    vec4 key = track(4.0 + float(k) * 2.0), value = track(5.0 + float(k) * 2.0);
    float span = key.x, op = key.y;
    if (span <= 0.0) {
      if (op == 1.0) velocity += value.xyz;
      else if (op == 2.0) velocity = value.xyz;
      else if (op == 3.0) velocity *= value.x;
    } else {
      float t = min(left, span), f = t / span;
      vec3 change = vec3(0.0);
      if (op == 1.0) change = value.xyz;
      else if (op == 2.0) change = value.xyz - velocity;
      else if (op == 3.0) change = velocity * (value.x - 1.0);
      displacement += velocity * t + change * (t * f * 0.5);
      velocity += change * f;
      left -= t;
    }
  }
  if (left > 0.0) displacement += velocity * left;
  vec3 iPosition = spawnPosition + displacement + origin.xyz * settings.y;
  float particleSize = spawnShape.x;
  float effectScale = clock.y;
  left = age;
  for (int k = 0; k < 8; k++) {
    if (float(k) >= counts.y || left < 0.0) break;
    vec4 key = track(20.0 + float(k) * 2.0);
    float value = track(21.0 + float(k) * 2.0).x;
    float f = key.x > 0.0 ? min(left / key.x, 1.0) : 1.0;
    if (key.y == 1.0) particleSize *= 1.0 + (value - 1.0) * f;
    else if (key.y == 2.0) particleSize += value * f;
    else if (key.y == 3.0) particleSize += (value - particleSize) * f;
    left -= key.x;
    if (f < 1.0) break;
  }
  vec4 iColor = spawnColor;
  left = age;
  for (int k = 0; k < 8; k++) {
    if (float(k) >= counts.z) break;
    vec4 key = track(36.0 + float(k) * 2.0), value = track(37.0 + float(k) * 2.0);
    float f = key.x > 0.0 ? min(left / key.x, 1.0) : 1.0;
    if (key.y == 1.0) iColor += (value - iColor) * f;
    else if (key.y == 2.0) iColor = clamp(iColor + value * f, 0.0, 1.0);
    left -= key.x;
    if (f < 1.0) break;
  }
  iColor *= origin.w;
  particleSize *= effectScale;
  float rotation = spawnShape.y + spawnShape.z * age;
  float speed = length(velocity);
  vec4 iShape = vec4(particleSize, rotation, speed * settings.z, settings.x);
  vec3 iAxis = vec3(0.0, 0.0, 1.0), iAxis2 = vec3(1.0, 0.0, 0.0);
  if (settings.x > 1.5) {
    iAxis = rotateBy(vec3(cos(rotation), 0.0, sin(rotation)), turn);
    iAxis2 = rotateBy(vec3(-sin(rotation), 0.0, cos(rotation)), turn);
  } else if (settings.x > 0.5 && speed > 0.0) iAxis = velocity / speed;
  vec4 iFrame = vec4(0.0, 0.0, 1.0, 1.0);
  if (atlas.x > 0.0) {
    float index = min(floor(age / atlas.x), atlas.z - 1.0);
    iFrame = vec4(mod(index, atlas.y), floor(index / atlas.y), 1.0, 1.0) / atlas.y;
  }
  // The CPU owns expiry: a float-rounded GPU lifetime comparison can remove
  // a still-live flash one frame early. Only invisible shapes are clipped here.
  if (particleSize <= 0.0 || (settings.x == 1.0 && speed < 0.0001)) {
    gl_Position = vec4(2.0, 2.0, 2.0, 1.0); return;
  }

    vColor = iColor;
    // Keep flat decals/shockwaves attached; soften volumetric sprites only.
    vSoftDistance = iShape.w > 1.5 ? 0.0 : clamp(iShape.x * 0.6, 0.04, 0.6);
    float size = iShape.x;
    vec2 uv = corner * 0.5 + 0.5;
    vec4 mv;
    if (iShape.w > 1.5) {
      vec3 p = iPosition + iAxis * (corner.x * size) + iAxis2 * (corner.y * size);
      mv = modelViewMatrix * vec4(p, 1.0);
    } else if (iShape.w > 0.5) {
      float head = corner.y * 0.5 + 0.5;
      vec3 dir = normalize(iAxis + vec3(0.0, 1e-6, 0.0));
      mv = modelViewMatrix * vec4(iPosition - dir * ((1.0 - head) * iShape.z), 1.0);
      vec3 along = normalize((modelViewMatrix * vec4(dir, 0.0)).xyz);
      vec3 side = normalize(cross(along, normalize(mv.xyz)) + vec3(1e-6, 0.0, 0.0));
      mv.xyz += side * corner.x * size * mix(iAxis2.x, 1.0, head);
      uv = vec2(1.0 - head, corner.x * 0.5 + 0.5);
    } else {
      mv = modelViewMatrix * vec4(iPosition, 1.0);
      float c = cos(iShape.y);
      float s = sin(iShape.y);
      mv.xy += vec2(c * corner.x - s * corner.y, s * corner.x + c * corner.y) * size;
    }
    vUv = iFrame.xy + uv * iFrame.zw;
    vDepth = -mv.z;
    gl_Position = projectionMatrix * mv;
  }
