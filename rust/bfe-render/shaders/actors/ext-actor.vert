uniform mat4 modelMatrix;
uniform mat4 modelViewMatrix;
uniform mat4 projectionMatrix;
uniform mat4 viewMatrix;
uniform mat3 normalMatrix;
uniform float time;
uniform float sunScale;
uniform vec2 ambientPull;
uniform float brightness;
#ifdef SCROLL
uniform vec2 scroll;
#endif
#ifdef USE_SKINNING
uniform mat4 bindMatrix;
uniform mat4 bindMatrixInverse;
uniform highp sampler2D boneTexture;
mat4 getBoneMatrix(const in float i) {
  int j = int( i ) * 4;
  int w = textureSize(boneTexture, 0).x;
  int x = j % w;
  int y = j / w;
  vec4 v1 = texelFetch(boneTexture, ivec2(x, y), 0);
  vec4 v2 = texelFetch(boneTexture, ivec2(x + 1, y), 0);
  vec4 v3 = texelFetch(boneTexture, ivec2(x + 2, y), 0);
  vec4 v4 = texelFetch(boneTexture, ivec2(x + 3, y), 0);
  return mat4(v1, v2, v3, v4);
}
#endif

#ifdef BAKED
in vec4 bakedLight;
out vec3 vBakedLight;
#ifndef NO_POINT
uniform vec4 zeroLights[ZERO_LIGHTS * 2];
uniform int zeroLightCount;
out vec3 vPoint;
#endif
#endif
in vec4 vertexTint;
out vec3 vTint;
in vec3 position;
in vec3 normal;
in vec2 uv;
#ifdef USE_SKINNING
in vec4 skinIndex;
in vec4 skinWeight;
#endif

out vec2 vUv;
out vec3 vNormalW;
out vec3 vWorld;
out float vDepth;

void main() {
  vec3 transformed = position;
  vec3 objectNormal = normal;
  #ifdef USE_SKINNING
    mat4 boneMatX = getBoneMatrix( skinIndex.x );
    mat4 boneMatY = getBoneMatrix( skinIndex.y );
    mat4 boneMatZ = getBoneMatrix( skinIndex.z );
    mat4 boneMatW = getBoneMatrix( skinIndex.w );
    mat4 skinMatrix = mat4(0.0);
    skinMatrix += skinWeight.x * boneMatX;
    skinMatrix += skinWeight.y * boneMatY;
    skinMatrix += skinWeight.z * boneMatZ;
    skinMatrix += skinWeight.w * boneMatW;
    skinMatrix = bindMatrixInverse * skinMatrix * bindMatrix;
    objectNormal = (skinMatrix * vec4(objectNormal, 0.0)).xyz;
    vec4 skinVertex = bindMatrix * vec4(transformed, 1.0);
    vec4 skinned = boneMatX * skinVertex * skinWeight.x
                 + boneMatY * skinVertex * skinWeight.y
                 + boneMatZ * skinVertex * skinWeight.z
                 + boneMatW * skinVertex * skinWeight.w;
    transformed = (bindMatrixInverse * skinned).xyz;
  #endif
  vec4 mvPosition = modelViewMatrix * vec4(transformed, 1.0);
  gl_Position = projectionMatrix * mvPosition;
  vDepth = -mvPosition.z;
  vWorld = (modelMatrix * vec4(transformed, 1.0)).xyz;
  vec3 transformedNormal = normalMatrix * objectNormal;
  vec3 wn = (vec4(transformedNormal, 0.0) * viewMatrix).xyz;
  vNormalW = length(wn) > 1e-5 ? normalize(wn) : vec3(0.0, 1.0, 0.0);
  #ifdef BAKED
    vBakedLight = bakedLight.rgb;
    #ifndef NO_POINT
    vPoint = vec3(0.0);
    for (int i = 0; i < ZERO_LIGHTS; i++) {
      if (i >= zeroLightCount) break;
      vec4 sphere = zeroLights[ 2 * i ];
      vec3 toLight = sphere.xyz - vWorld;
      float range = sphere.w;
      float d2 = dot(toLight, toLight);
      if (range <= 0.0 || d2 < range * range) {
        float reach = range > 0.0 ? max(1.0 - d2 / (range * range), 0.0) : 1.0;
        vPoint += max(dot(vNormalW, toLight * inversesqrt(max(d2, 0.0001))), 0.0) * reach * zeroLights[ 2 * i + 1 ].rgb;
      }
    }
    #endif
  #endif
  vTint = vertexTint.rgb;
  vUv = uv;
  #ifdef SCROLL
    vUv -= scroll * time;
  #endif
}
