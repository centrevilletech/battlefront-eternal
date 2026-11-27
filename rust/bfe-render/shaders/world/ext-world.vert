uniform mat4 projectionMatrix;
uniform mat4 viewMatrix;
uniform vec3 cameraPosition;
uniform float time;
uniform vec3 sunDirection;
uniform vec3 sunColor;
uniform vec3 ambient;
uniform vec3 ambientTop;
uniform vec3 ambientBottom;
uniform float sunScale;
uniform vec2 ambientPull;
#ifdef SCROLL
uniform vec2 scroll;
#endif
#if !defined(NO_POINT) && !defined(DYNAMIC) && !defined(FAR)
uniform vec4 zeroLights[ZERO_LIGHTS * 2];
uniform int zeroLightCount;
#endif

in vec3 position;
in vec3 normal;
#if !defined(TERRAIN) && defined(USE_MAP)
in vec2 uv;
#endif
#ifdef HAS_COLOR
in vec4 color;
#endif
#ifdef BUMP
in vec3 tangent;
in vec3 binormal;
#endif
#ifdef TERRAIN_LAYERS
in vec4 weights0;
in vec4 weights1;
out vec4 vWeights0;
out vec4 vWeights1;
#endif

out vec2 vUv;
out vec4 vColor;
out vec3 vNormalW;
out vec3 vWorld;
out vec3 vPoint;
out float vDepth;
#ifdef BUMP
out vec3 vTangent;
out vec3 vBinormal;
#endif

void main() {
  vec4 mvPosition = viewMatrix * vec4(position, 1.0);
  gl_Position = projectionMatrix * mvPosition;
  vDepth = -mvPosition.z;
  vWorld = position;
  vec3 n = length(normal) > 1e-5 ? normalize(normal) : vec3(0.0, 1.0, 0.0);
  vNormalW = n;
  #if !defined(TERRAIN) && defined(USE_MAP)
    vUv = uv;
    #ifdef SCROLL
      vUv -= scroll * time;
    #endif
  #else
    vUv = vec2(0.0);
  #endif
  #ifdef HAS_COLOR
    vColor = color;
  #else
    vColor = vec4(1.0);
  #endif
  #ifdef BUMP
    vTangent = tangent;
    vBinormal = binormal;
  #endif
  #ifdef TERRAIN_LAYERS
    vWeights0 = weights0;
    vWeights1 = weights1;
  #endif
  vPoint = vec3(0.0);
  #if !defined(NO_POINT) && !defined(DYNAMIC) && !defined(FAR)
    for (int i = 0; i < ZERO_LIGHTS; i++) {
      if (i >= zeroLightCount) break;
      vec4 sphere = zeroLights[2 * i];
      vec3 toLight = sphere.xyz - vWorld;
      float range = sphere.w;
      float d2 = dot(toLight, toLight);
      if (range <= 0.0 || d2 < range * range) {
        float reach = range > 0.0 ? max(1.0 - d2 / (range * range), 0.0) : 1.0;
        vPoint += max(dot(n, toLight * inversesqrt(max(d2, 0.0001))), 0.0) * reach * zeroLights[2 * i + 1].rgb;
      }
    }
  #endif
}
