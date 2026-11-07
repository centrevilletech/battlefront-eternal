#endif
#if 0 > 0
uniform sampler2D spotShadowMap[ 0 ];
struct SpotLightShadow {
float shadowIntensity;
float shadowBias;
float shadowNormalBias;
float shadowRadius;
vec2 shadowMapSize;
};
uniform SpotLightShadow spotLightShadows[ 0 ];
#endif
#if 0 > 0
uniform sampler2D pointShadowMap[ 0 ];
varying vec4 vPointShadowCoord[ 0 ];
struct PointLightShadow {
float shadowIntensity;
float shadowBias;
float shadowNormalBias;
float shadowRadius;
vec2 shadowMapSize;
float shadowCameraNear;
float shadowCameraFar;
};
uniform PointLightShadow pointLightShadows[ 0 ];
#endif
float texture2DCompare( sampler2D depths, vec2 uv, float compare ) {
return step( compare, unpackRGBAToDepth( texture2D( depths, uv ) ) );
}
vec2 texture2DDistribution( sampler2D shadow, vec2 uv ) {
return unpackRGBATo2Half( texture2D( shadow, uv ) );
}
float VSMShadow (sampler2D shadow, vec2 uv, float compare ){
float occlusion = 1.0;
vec2 distribution = texture2DDistribution( shadow, uv );
float hard_shadow = step( compare , distribution.x );
if (hard_shadow != 1.0 ) {
float distance = compare - distribution.x ;
float variance = max( 0.00000, distribution.y * distribution.y );
float softness_probability = variance / (variance + distance * distance );			softness_probability = clamp( ( softness_probability - 0.3 ) / ( 0.95 - 0.3 ), 0.0, 1.0 );			occlusion = clamp( max( hard_shadow, softness_probability ), 0.0, 1.0 );
}
return occlusion;
}
float getShadow( sampler2D shadowMap, vec2 shadowMapSize, float shadowIntensity, float shadowBias, float shadowRadius, vec4 shadowCoord ) {
float shadow = 1.0;
shadowCoord.xyz /= shadowCoord.w;
shadowCoord.z += shadowBias;
bool inFrustum = shadowCoord.x >= 0.0 && shadowCoord.x <= 1.0 && shadowCoord.y >= 0.0 && shadowCoord.y <= 1.0;
bool frustumTest = inFrustum && shadowCoord.z <= 1.0;
if ( frustumTest ) {
#if defined( SHADOWMAP_TYPE_PCF )
vec2 texelSize = vec2( 1.0 ) / shadowMapSize;
float dx0 = - texelSize.x * shadowRadius;
float dy0 = - texelSize.y * shadowRadius;
float dx1 = + texelSize.x * shadowRadius;
float dy1 = + texelSize.y * shadowRadius;
float dx2 = dx0 / 2.0;
float dy2 = dy0 / 2.0;
float dx3 = dx1 / 2.0;
float dy3 = dy1 / 2.0;
shadow = (
texture2DCompare( shadowMap, shadowCoord.xy + vec2( dx0, dy0 ), shadowCoord.z ) +
texture2DCompare( shadowMap, shadowCoord.xy + vec2( 0.0, dy0 ), shadowCoord.z ) +
texture2DCompare( shadowMap, shadowCoord.xy + vec2( dx1, dy0 ), shadowCoord.z ) +
texture2DCompare( shadowMap, shadowCoord.xy + vec2( dx2, dy2 ), shadowCoord.z ) +
texture2DCompare( shadowMap, shadowCoord.xy + vec2( 0.0, dy2 ), shadowCoord.z ) +
texture2DCompare( shadowMap, shadowCoord.xy + vec2( dx3, dy2 ), shadowCoord.z ) +
texture2DCompare( shadowMap, shadowCoord.xy + vec2( dx0, 0.0 ), shadowCoord.z ) +
texture2DCompare( shadowMap, shadowCoord.xy + vec2( dx2, 0.0 ), shadowCoord.z ) +
texture2DCompare( shadowMap, shadowCoord.xy, shadowCoord.z ) +
texture2DCompare( shadowMap, shadowCoord.xy + vec2( dx3, 0.0 ), shadowCoord.z ) +
texture2DCompare( shadowMap, shadowCoord.xy + vec2( dx1, 0.0 ), shadowCoord.z ) +
texture2DCompare( shadowMap, shadowCoord.xy + vec2( dx2, dy3 ), shadowCoord.z ) +
texture2DCompare( shadowMap, shadowCoord.xy + vec2( 0.0, dy3 ), shadowCoord.z ) +
texture2DCompare( shadowMap, shadowCoord.xy + vec2( dx3, dy3 ), shadowCoord.z ) +
texture2DCompare( shadowMap, shadowCoord.xy + vec2( dx0, dy1 ), shadowCoord.z ) +
texture2DCompare( shadowMap, shadowCoord.xy + vec2( 0.0, dy1 ), shadowCoord.z ) +
texture2DCompare( shadowMap, shadowCoord.xy + vec2( dx1, dy1 ), shadowCoord.z )
) * ( 1.0 / 17.0 );
#elif defined( SHADOWMAP_TYPE_PCF_SOFT )
vec2 texelSize = vec2( 1.0 ) / shadowMapSize;
float dx = texelSize.x;
float dy = texelSize.y;
vec2 uv = shadowCoord.xy;
vec2 f = fract( uv * shadowMapSize + 0.5 );
uv -= f * texelSize;
shadow = (
texture2DCompare( shadowMap, uv, shadowCoord.z ) +
texture2DCompare( shadowMap, uv + vec2( dx, 0.0 ), shadowCoord.z ) +
texture2DCompare( shadowMap, uv + vec2( 0.0, dy ), shadowCoord.z ) +
texture2DCompare( shadowMap, uv + texelSize, shadowCoord.z ) +
mix( texture2DCompare( shadowMap, uv + vec2( -dx, 0.0 ), shadowCoord.z ),
texture2DCompare( shadowMap, uv + vec2( 2.0 * dx, 0.0 ), shadowCoord.z ),
f.x ) +
mix( texture2DCompare( shadowMap, uv + vec2( -dx, dy ), shadowCoord.z ),
texture2DCompare( shadowMap, uv + vec2( 2.0 * dx, dy ), shadowCoord.z ),
f.x ) +
mix( texture2DCompare( shadowMap, uv + vec2( 0.0, -dy ), shadowCoord.z ),
texture2DCompare( shadowMap, uv + vec2( 0.0, 2.0 * dy ), shadowCoord.z ),
f.y ) +
mix( texture2DCompare( shadowMap, uv + vec2( dx, -dy ), shadowCoord.z ),
texture2DCompare( shadowMap, uv + vec2( dx, 2.0 * dy ), shadowCoord.z ),
f.y ) +
mix( mix( texture2DCompare( shadowMap, uv + vec2( -dx, -dy ), shadowCoord.z ),
texture2DCompare( shadowMap, uv + vec2( 2.0 * dx, -dy ), shadowCoord.z ),
f.x ),
mix( texture2DCompare( shadowMap, uv + vec2( -dx, 2.0 * dy ), shadowCoord.z ),
texture2DCompare( shadowMap, uv + vec2( 2.0 * dx, 2.0 * dy ), shadowCoord.z ),
f.x ),
f.y )
) * ( 1.0 / 9.0 );
#elif defined( SHADOWMAP_TYPE_VSM )
shadow = VSMShadow( shadowMap, shadowCoord.xy, shadowCoord.z );
#else
shadow = texture2DCompare( shadowMap, shadowCoord.xy, shadowCoord.z );
#endif
}
return mix( 1.0, shadow, shadowIntensity );
}
vec2 cubeToUV( vec3 v, float texelSizeY ) {
vec3 absV = abs( v );
float scaleToCube = 1.0 / max( absV.x, max( absV.y, absV.z ) );
absV *= scaleToCube;
v *= scaleToCube * ( 1.0 - 2.0 * texelSizeY );
vec2 planar = v.xy;
float almostATexel = 1.5 * texelSizeY;
float almostOne = 1.0 - almostATexel;
if ( absV.z >= almostOne ) {
if ( v.z > 0.0 )
planar.x = 4.0 - v.x;
} else if ( absV.x >= almostOne ) {
float signX = sign( v.x );
planar.x = v.z * signX + 2.0 * signX;
} else if ( absV.y >= almostOne ) {
float signY = sign( v.y );
planar.x = v.x + 2.0 * signY + 2.0;
planar.y = v.z * signY - 2.0;
}
return vec2( 0.125, 0.25 ) * planar + vec2( 0.375, 0.75 );
}
float getPointShadow( sampler2D shadowMap, vec2 shadowMapSize, float shadowIntensity, float shadowBias, float shadowRadius, vec4 shadowCoord, float shadowCameraNear, float shadowCameraFar ) {
float shadow = 1.0;
vec3 lightToPosition = shadowCoord.xyz;
float lightToPositionLength = length( lightToPosition );
if ( lightToPositionLength - shadowCameraFar <= 0.0 && lightToPositionLength - shadowCameraNear >= 0.0 ) {
float dp = ( lightToPositionLength - shadowCameraNear ) / ( shadowCameraFar - shadowCameraNear );			dp += shadowBias;
vec3 bd3D = normalize( lightToPosition );
vec2 texelSize = vec2( 1.0 ) / ( shadowMapSize * vec2( 4.0, 2.0 ) );
#if defined( SHADOWMAP_TYPE_PCF ) || defined( SHADOWMAP_TYPE_PCF_SOFT ) || defined( SHADOWMAP_TYPE_VSM )
vec2 offset = vec2( - 1, 1 ) * shadowRadius * texelSize.y;
shadow = (
texture2DCompare( shadowMap, cubeToUV( bd3D + offset.xyy, texelSize.y ), dp ) +
texture2DCompare( shadowMap, cubeToUV( bd3D + offset.yyy, texelSize.y ), dp ) +
texture2DCompare( shadowMap, cubeToUV( bd3D + offset.xyx, texelSize.y ), dp ) +
texture2DCompare( shadowMap, cubeToUV( bd3D + offset.yyx, texelSize.y ), dp ) +
texture2DCompare( shadowMap, cubeToUV( bd3D, texelSize.y ), dp ) +
texture2DCompare( shadowMap, cubeToUV( bd3D + offset.xxy, texelSize.y ), dp ) +
texture2DCompare( shadowMap, cubeToUV( bd3D + offset.yxy, texelSize.y ), dp ) +
texture2DCompare( shadowMap, cubeToUV( bd3D + offset.xxx, texelSize.y ), dp ) +
texture2DCompare( shadowMap, cubeToUV( bd3D + offset.yxx, texelSize.y ), dp )
) * ( 1.0 / 9.0 );
#else
shadow = texture2DCompare( shadowMap, cubeToUV( bd3D, texelSize.y ), dp );
#endif
}
return mix( 1.0, shadow, shadowIntensity );
}
#endif
uniform sampler2D map;
uniform vec3 sunDirection;
uniform vec3 sunColor;
uniform vec3 characterAmbient;
uniform vec3 ambient;
uniform float sunScale;
uniform float brightness;
uniform vec2 ambientPull;
#ifdef DYNAMIC
uniform vec4 localLightPositions[4];
uniform vec3 localLightColors[4];
uniform float localLightCount;
#if !defined( NO_POINT ) && !defined( FAR )
uniform vec4 zeroLights[ZERO_LIGHTS * 2];
uniform int zeroLightCount;
#endif
#endif
uniform vec2 fade;
uniform sampler2D farMap;
uniform vec2 resolution;
uniform vec3 fogColor;
uniform vec2 fogRange;
uniform vec2 cascades[4];
uniform float cascadeNear;
uniform float cascadeFar;
uniform float bump;
uniform float aaSamples;
uniform float farAaSamples;
uniform float shadowStrength;
uniform float terrainShade;
uniform vec4 specular;
#ifdef BUMP
uniform sampler2D bumpMap;
varying vec3 vTangent;
varying vec3 vBinormal;
#endif
#ifdef TERRAIN
uniform sampler2D layer1Map;
uniform sampler2D detailMap;
uniform vec2 layerScales;
uniform vec2 farOrigin;
uniform vec3 bakedLit;
#endif
#ifdef TERRAIN_LAYERS
// Every map layer a patch paints with, summed by per-vertex weight.
uniform vec4 layerScale0;
uniform vec4 layerScale1;
varying vec4 vWeights0;
varying vec4 vWeights1;
uniform sampler2D layerMap0;
#if TERRAIN_LAYERS > 1
uniform sampler2D layerMap1;
#endif
#if TERRAIN_LAYERS > 2
uniform sampler2D layerMap2;
#endif
#if TERRAIN_LAYERS > 3
uniform sampler2D layerMap3;
#endif
#if TERRAIN_LAYERS > 4
uniform sampler2D layerMap4;
#endif
#if TERRAIN_LAYERS > 5
uniform sampler2D layerMap5;
#endif
#if TERRAIN_LAYERS > 6
uniform sampler2D layerMap6;
#endif
#if TERRAIN_LAYERS > 7
uniform sampler2D layerMap7;
#endif
// Gradients come from the shared world position, so a layer can be
// skipped where it has no weight without breaking its mip selection.
void paint(inout vec4 sum, sampler2D layer, float weight, float scale, vec2 ground, vec2 dx, vec2 dy) {
if (weight > 0.002) sum += weight * textureGrad(layer, ground / scale, dx / scale, dy / scale);
}
vec4 paintedGround(vec2 ground) {
vec2 dx = dFdx(ground);
vec2 dy = dFdy(ground);
vec4 sum = vec4(0.0);
paint(sum, layerMap0, vWeights0.x, layerScale0.x, ground, dx, dy);
#if TERRAIN_LAYERS > 1
paint(sum, layerMap1, vWeights0.y, layerScale0.y, ground, dx, dy);
#endif
#if TERRAIN_LAYERS > 2
paint(sum, layerMap2, vWeights0.z, layerScale0.z, ground, dx, dy);
#endif
#if TERRAIN_LAYERS > 3
paint(sum, layerMap3, vWeights0.w, layerScale0.w, ground, dx, dy);
#endif
#if TERRAIN_LAYERS > 4
paint(sum, layerMap4, vWeights1.x, layerScale1.x, ground, dx, dy);
#endif
#if TERRAIN_LAYERS > 5
paint(sum, layerMap5, vWeights1.y, layerScale1.y, ground, dx, dy);
#endif
#if TERRAIN_LAYERS > 6
paint(sum, layerMap6, vWeights1.z, layerScale1.z, ground, dx, dy);
#endif
#if TERRAIN_LAYERS > 7
paint(sum, layerMap7, vWeights1.w, layerScale1.w, ground, dx, dy);
#endif
float total = dot(vWeights0, vec4(1.0)) + dot(vWeights1, vec4(1.0));
return sum / max(total, 0.004);
}
#endif
varying vec2 vUv;
varying vec4 vColor;
varying vec3 vAmbient;
varying vec3 vSun;
varying vec3 vPoint;
varying vec3 vNormalW;
varying vec3 vWorld;
varying float vDepth;
// Receiver-plane PCF compares each tap against the depth of THIS triangle
// at that tap. A constant normal offset can smear low-poly animated faces
// and is too small for distant cascades. The footprint is a 4x4 tent: the
// texels base-1 to base+2 on each axis, weighted (1-f, 1, 1, f)/3, which
// is a 3x3 box of bilinear taps. The GPU's comparison sampler returns a
// bilinear blend of four texel comparisons, so two taps an axis (weights
// 2-f and 1+f, placed where their bilinear blends sum to the tent) make
// the same sixteen comparisons in four fetches. Each fetch compares its
// four texels against the receiver plane at the fetch.
