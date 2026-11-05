#version 300 es
#define varying in
layout(location = 0) out highp vec4 pc_fragColor;
#define gl_FragColor pc_fragColor
#define gl_FragDepthEXT gl_FragDepth
#define texture2D texture
#define textureCube texture
#define texture2DProj textureProj
#define texture2DLodEXT textureLod
#define texture2DProjLodEXT textureProjLod
#define textureCubeLodEXT textureLod
#define texture2DGradEXT textureGrad
#define texture2DProjGradEXT textureProjGrad
#define textureCubeGradEXT textureGrad
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
#define SHADER_NAME zero:terrain
#define ZERO_LIGHTS 16
#define TERRAIN 
#define HAS_COLOR 
#define WORLD_SPACE 
#define TERRAIN_LAYERS 6
#define USE_MAP
uniform mat4 viewMatrix;
uniform vec3 cameraPosition;
uniform bool isOrthographic;
#define OPAQUE
vec4 LinearTransferOETF( in vec4 value ) {
	return value;
}
vec4 sRGBTransferEOTF( in vec4 value ) {
	return vec4( mix( pow( value.rgb * 0.9478672986 + vec3( 0.0521327014 ), vec3( 2.4 ) ), value.rgb * 0.0773993808, vec3( lessThanEqual( value.rgb, vec3( 0.04045 ) ) ) ), value.a );
}
vec4 sRGBTransferOETF( in vec4 value ) {
	return vec4( mix( pow( value.rgb, vec3( 0.41666 ) ) * 1.055 - vec3( 0.055 ), value.rgb * 12.92, vec3( lessThanEqual( value.rgb, vec3( 0.0031308 ) ) ) ), value.a );
}
vec4 linearToOutputTexel( vec4 value ) {
	return LinearTransferOETF( vec4( value.rgb * mat3( 1.0000,-0.0000,-0.0000,-0.0000,1.0000,0.0000,0.0000,0.0000,1.0000 ), value.a ) );
}
float luminance( const in vec3 rgb ) {
	const vec3 weights = vec3( 0.2126, 0.7152, 0.0722 );
	return dot( weights, rgb );
}


#define PI 3.141592653589793
#define PI2 6.283185307179586
#define PI_HALF 1.5707963267948966
#define RECIPROCAL_PI 0.3183098861837907
#define RECIPROCAL_PI2 0.15915494309189535
#define EPSILON 1e-6
#ifndef saturate
#define saturate( a ) clamp( a, 0.0, 1.0 )
#endif
#define whiteComplement( a ) ( 1.0 - saturate( a ) )
float pow2( const in float x ) { return x*x; }
vec3 pow2( const in vec3 x ) { return x*x; }
float pow3( const in float x ) { return x*x*x; }
float pow4( const in float x ) { float x2 = x*x; return x2*x2; }
float max3( const in vec3 v ) { return max( max( v.x, v.y ), v.z ); }
float average( const in vec3 v ) { return dot( v, vec3( 0.3333333 ) ); }
highp float rand( const in vec2 uv ) {
	const highp float a = 12.9898, b = 78.233, c = 43758.5453;
	highp float dt = dot( uv.xy, vec2( a,b ) ), sn = mod( dt, PI );
	return fract( sin( sn ) * c );
}
#ifdef HIGH_PRECISION
	float precisionSafeLength( vec3 v ) { return length( v ); }
#else
	float precisionSafeLength( vec3 v ) {
		float maxComponent = max3( abs( v ) );
		return length( v / maxComponent ) * maxComponent;
	}
#endif
struct IncidentLight {
	vec3 color;
	vec3 direction;
	bool visible;
};
struct ReflectedLight {
	vec3 directDiffuse;
	vec3 directSpecular;
	vec3 indirectDiffuse;
	vec3 indirectSpecular;
};
#ifdef USE_ALPHAHASH
	varying vec3 vPosition;
#endif
vec3 transformDirection( in vec3 dir, in mat4 matrix ) {
	return normalize( ( matrix * vec4( dir, 0.0 ) ).xyz );
}
vec3 inverseTransformDirection( in vec3 dir, in mat4 matrix ) {
	return normalize( ( vec4( dir, 0.0 ) * matrix ).xyz );
}
mat3 transposeMat3( const in mat3 m ) {
	mat3 tmp;
	tmp[ 0 ] = vec3( m[ 0 ].x, m[ 1 ].x, m[ 2 ].x );
	tmp[ 1 ] = vec3( m[ 0 ].y, m[ 1 ].y, m[ 2 ].y );
	tmp[ 2 ] = vec3( m[ 0 ].z, m[ 1 ].z, m[ 2 ].z );
	return tmp;
}
bool isPerspectiveMatrix( mat4 m ) {
	return m[ 2 ][ 3 ] == - 1.0;
}
vec2 equirectUv( in vec3 dir ) {
	float u = atan( dir.z, dir.x ) * RECIPROCAL_PI2 + 0.5;
	float v = asin( clamp( dir.y, - 1.0, 1.0 ) ) * RECIPROCAL_PI + 0.5;
	return vec2( u, v );
}
vec3 BRDF_Lambert( const in vec3 diffuseColor ) {
	return RECIPROCAL_PI * diffuseColor;
}
vec3 F_Schlick( const in vec3 f0, const in float f90, const in float dotVH ) {
	float fresnel = exp2( ( - 5.55473 * dotVH - 6.98316 ) * dotVH );
	return f0 * ( 1.0 - fresnel ) + ( f90 * fresnel );
}
float F_Schlick( const in float f0, const in float f90, const in float dotVH ) {
	float fresnel = exp2( ( - 5.55473 * dotVH - 6.98316 ) * dotVH );
	return f0 * ( 1.0 - fresnel ) + ( f90 * fresnel );
} // validated
vec3 packNormalToRGB( const in vec3 normal ) {
	return normalize( normal ) * 0.5 + 0.5;
}
vec3 unpackRGBToNormal( const in vec3 rgb ) {
	return 2.0 * rgb.xyz - 1.0;
}
const float PackUpscale = 256. / 255.;const float UnpackDownscale = 255. / 256.;const float ShiftRight8 = 1. / 256.;
const float Inv255 = 1. / 255.;
const vec4 PackFactors = vec4( 1.0, 256.0, 256.0 * 256.0, 256.0 * 256.0 * 256.0 );
const vec2 UnpackFactors2 = vec2( UnpackDownscale, 1.0 / PackFactors.g );
const vec3 UnpackFactors3 = vec3( UnpackDownscale / PackFactors.rg, 1.0 / PackFactors.b );
const vec4 UnpackFactors4 = vec4( UnpackDownscale / PackFactors.rgb, 1.0 / PackFactors.a );
vec4 packDepthToRGBA( const in float v ) {
	if( v <= 0.0 )
		return vec4( 0., 0., 0., 0. );
	if( v >= 1.0 )
		return vec4( 1., 1., 1., 1. );
	float vuf;
	float af = modf( v * PackFactors.a, vuf );
	float bf = modf( vuf * ShiftRight8, vuf );
	float gf = modf( vuf * ShiftRight8, vuf );
	return vec4( vuf * Inv255, gf * PackUpscale, bf * PackUpscale, af );
}
vec3 packDepthToRGB( const in float v ) {
	if( v <= 0.0 )
		return vec3( 0., 0., 0. );
	if( v >= 1.0 )
		return vec3( 1., 1., 1. );
	float vuf;
	float bf = modf( v * PackFactors.b, vuf );
	float gf = modf( vuf * ShiftRight8, vuf );
	return vec3( vuf * Inv255, gf * PackUpscale, bf );
}
vec2 packDepthToRG( const in float v ) {
	if( v <= 0.0 )
		return vec2( 0., 0. );
	if( v >= 1.0 )
		return vec2( 1., 1. );
	float vuf;
	float gf = modf( v * 256., vuf );
	return vec2( vuf * Inv255, gf );
}
float unpackRGBAToDepth( const in vec4 v ) {
	return dot( v, UnpackFactors4 );
}
float unpackRGBToDepth( const in vec3 v ) {
	return dot( v, UnpackFactors3 );
}
float unpackRGToDepth( const in vec2 v ) {
	return v.r * UnpackFactors2.r + v.g * UnpackFactors2.g;
}
vec4 pack2HalfToRGBA( const in vec2 v ) {
	vec4 r = vec4( v.x, fract( v.x * 255.0 ), v.y, fract( v.y * 255.0 ) );
	return vec4( r.x - r.y / 255.0, r.y, r.z - r.w / 255.0, r.w );
}
vec2 unpackRGBATo2Half( const in vec4 v ) {
	return vec2( v.x + ( v.y / 255.0 ), v.z + ( v.w / 255.0 ) );
}
float viewZToOrthographicDepth( const in float viewZ, const in float near, const in float far ) {
	return ( viewZ + near ) / ( near - far );
}
float orthographicDepthToViewZ( const in float depth, const in float near, const in float far ) {
	return depth * ( near - far ) - near;
}
float viewZToPerspectiveDepth( const in float viewZ, const in float near, const in float far ) {
	return ( ( near + viewZ ) * far ) / ( ( far - near ) * viewZ );
}
float perspectiveDepthToViewZ( const in float depth, const in float near, const in float far ) {
	return ( near * far ) / ( ( far - near ) * depth - far );
}
uniform bool receiveShadow;
uniform vec3 ambientLightColor;
#if defined( USE_LIGHT_PROBES )
	uniform vec3 lightProbe[ 9 ];
#endif
vec3 shGetIrradianceAt( in vec3 normal, in vec3 shCoefficients[ 9 ] ) {
	float x = normal.x, y = normal.y, z = normal.z;
	vec3 result = shCoefficients[ 0 ] * 0.886227;
	result += shCoefficients[ 1 ] * 2.0 * 0.511664 * y;
	result += shCoefficients[ 2 ] * 2.0 * 0.511664 * z;
	result += shCoefficients[ 3 ] * 2.0 * 0.511664 * x;
	result += shCoefficients[ 4 ] * 2.0 * 0.429043 * x * y;
	result += shCoefficients[ 5 ] * 2.0 * 0.429043 * y * z;
	result += shCoefficients[ 6 ] * ( 0.743125 * z * z - 0.247708 );
	result += shCoefficients[ 7 ] * 2.0 * 0.429043 * x * z;
	result += shCoefficients[ 8 ] * 0.429043 * ( x * x - y * y );
	return result;
}
vec3 getLightProbeIrradiance( const in vec3 lightProbe[ 9 ], const in vec3 normal ) {
	vec3 worldNormal = inverseTransformDirection( normal, viewMatrix );
	vec3 irradiance = shGetIrradianceAt( worldNormal, lightProbe );
	return irradiance;
}
vec3 getAmbientLightIrradiance( const in vec3 ambientLightColor ) {
	vec3 irradiance = ambientLightColor;
	return irradiance;
}
float getDistanceAttenuation( const in float lightDistance, const in float cutoffDistance, const in float decayExponent ) {
	float distanceFalloff = 1.0 / max( pow( lightDistance, decayExponent ), 0.01 );
	if ( cutoffDistance > 0.0 ) {
		distanceFalloff *= pow2( saturate( 1.0 - pow4( lightDistance / cutoffDistance ) ) );
	}
	return distanceFalloff;
}
float getSpotAttenuation( const in float coneCosine, const in float penumbraCosine, const in float angleCosine ) {
	return smoothstep( coneCosine, penumbraCosine, angleCosine );
}
#if 0 > 0
	struct DirectionalLight {
		vec3 direction;
		vec3 color;
	};
	uniform DirectionalLight directionalLights[ 0 ];
	void getDirectionalLightInfo( const in DirectionalLight directionalLight, out IncidentLight light ) {
		light.color = directionalLight.color;
		light.direction = directionalLight.direction;
		light.visible = true;
	}
#endif
#if 8 > 0
	struct PointLight {
		vec3 position;
		vec3 color;
		float distance;
		float decay;
	};
	uniform PointLight pointLights[ 8 ];
	void getPointLightInfo( const in PointLight pointLight, const in vec3 geometryPosition, out IncidentLight light ) {
		vec3 lVector = pointLight.position - geometryPosition;
		light.direction = normalize( lVector );
		float lightDistance = length( lVector );
		light.color = pointLight.color;
		light.color *= getDistanceAttenuation( lightDistance, pointLight.distance, pointLight.decay );
		light.visible = ( light.color != vec3( 0.0 ) );
	}
#endif
#if 0 > 0
	struct SpotLight {
		vec3 position;
		vec3 direction;
		vec3 color;
		float distance;
		float decay;
		float coneCos;
		float penumbraCos;
	};
	uniform SpotLight spotLights[ 0 ];
	void getSpotLightInfo( const in SpotLight spotLight, const in vec3 geometryPosition, out IncidentLight light ) {
		vec3 lVector = spotLight.position - geometryPosition;
		light.direction = normalize( lVector );
		float angleCos = dot( light.direction, spotLight.direction );
		float spotAttenuation = getSpotAttenuation( spotLight.coneCos, spotLight.penumbraCos, angleCos );
		if ( spotAttenuation > 0.0 ) {
			float lightDistance = length( lVector );
			light.color = spotLight.color * spotAttenuation;
			light.color *= getDistanceAttenuation( lightDistance, spotLight.distance, spotLight.decay );
			light.visible = ( light.color != vec3( 0.0 ) );
		} else {
			light.color = vec3( 0.0 );
			light.visible = false;
		}
	}
#endif
#if 0 > 0
	struct RectAreaLight {
		vec3 color;
		vec3 position;
		vec3 halfWidth;
		vec3 halfHeight;
	};
	uniform sampler2D ltc_1;	uniform sampler2D ltc_2;
	uniform RectAreaLight rectAreaLights[ 0 ];
#endif
#if 0 > 0
	struct HemisphereLight {
		vec3 direction;
		vec3 skyColor;
		vec3 groundColor;
	};
	uniform HemisphereLight hemisphereLights[ 0 ];
	vec3 getHemisphereLightIrradiance( const in HemisphereLight hemiLight, const in vec3 normal ) {
		float dotNL = dot( normal, hemiLight.direction );
		float hemiDiffuseWeight = 0.5 * dotNL + 0.5;
		vec3 irradiance = mix( hemiLight.groundColor, hemiLight.skyColor, hemiDiffuseWeight );
		return irradiance;
	}
#endif
#if 0 > 0
	varying vec4 vSpotLightCoord[ 0 ];
#endif
#if 0 > 0
	uniform sampler2D spotLightMap[ 0 ];
#endif
#ifdef USE_SHADOWMAP
	#if 0 > 0
		uniform sampler2D directionalShadowMap[ 0 ];
		varying vec4 vDirectionalShadowCoord[ 0 ];
		struct DirectionalLightShadow {
			float shadowIntensity;
			float shadowBias;
			float shadowNormalBias;
			float shadowRadius;
			vec2 shadowMapSize;
		};
		uniform DirectionalLightShadow directionalLightShadows[ 0 ];
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
  #if defined( USE_SHADOWMAP ) && 0 > 0
  uniform highp sampler2DShadow cascadeMaps[ 0 ];
  // The map coordinates come from the world position here, for the one or
  // two cascades a pixel reads, instead of four vec4 varyings a vertex.
  uniform mat4 directionalShadowMatrix[ 0 ];
  uniform mat3 cascadePlanes[ 0 ];
  float planeShadow(highp sampler2DShadow depthMap, vec2 size, float bias, vec3 coord, vec2 gradient) {
    if (coord.z < 0.0 || coord.z > 1.0 || any(lessThan(coord.xy, vec2(0.0))) || any(greaterThan(coord.xy, vec2(1.0)))) return 1.0;
    vec2 pixel = coord.xy * size - 0.5;
    vec2 base = floor(pixel);
    vec2 f = fract(pixel);
    // A fetch's four texels share its reference depth, so the plane is off
    // by up to a texel's slope at them. Where that is more than 6 cm (the
    // depth range is 3000 m), as on a wall nearly edge-on to the sun whose
    // own back face is the nearest caster, compare every texel on the plane.
    vec2 slope = abs(gradient) / size;
    if (max(slope.x, slope.y) > 2e-5) {
      vec4 wx = vec4(1.0 - f.x, 1.0, 1.0, f.x) / 3.0;
      vec4 wy = vec4(1.0 - f.y, 1.0, 1.0, f.y) / 3.0;
      float result = 0.0;
      for (int y = 0; y < 4; y++) {
        for (int x = 0; x < 4; x++) {
          vec2 uv = (base + vec2(float(x), float(y)) - 0.5) / size;
          float planeOffset = clamp(dot(gradient, uv - coord.xy), -0.002, 0.002);
          result += wx[x] * wy[y] * texture(depthMap, vec3(uv, coord.z + planeOffset + bias));
        }
      }
      return result;
    }
    vec2 wa = 2.0 - f;
    vec2 wb = 1.0 + f;
    vec2 a = (base - 0.5 + 1.0 / wa) / size;
    vec2 b = (base + 1.5 + f / wb) / size;
    float reference = coord.z + bias;
    float result = wa.x * wa.y * texture(depthMap, vec3(a, reference + clamp(dot(gradient, a - coord.xy), -0.002, 0.002)));
    result += wb.x * wa.y * texture(depthMap, vec3(b.x, a.y, reference + clamp(dot(gradient, vec2(b.x, a.y) - coord.xy), -0.002, 0.002)));
    result += wa.x * wb.y * texture(depthMap, vec3(a.x, b.y, reference + clamp(dot(gradient, vec2(a.x, b.y) - coord.xy), -0.002, 0.002)));
    result += wb.x * wb.y * texture(depthMap, vec3(b, reference + clamp(dot(gradient, b - coord.xy), -0.002, 0.002)));
    return result / 9.0;
  }
  #endif

  // The sun's shadow from the cascade covering this depth: 1 lit, 0 dark.
  float sunShadow() {
    float shadow = 1.0;
    #if defined( USE_SHADOWMAP ) && 0 > 0
      if (receiveShadow) {
        // CSM's split fractions are distances divided by far, not far-near.
        float linearDepth = vDepth / cascadeFar;
        // The receiver plane is the triangle's: its normal from the world
        // position's derivatives, taken here, outside the per-pixel cascade
        // branches, where every fragment of the quad contributes.
        vec3 facet = cross(dFdx(vWorld), dFdy(vWorld));
        DirectionalLightShadow cascadeShadow;
        vec2 interval;
        vec2 previous;
        float weight;
        vec4 projected;
        vec3 coord;
        vec3 plane;
        vec2 gradient;
        
        // Shadows thin out toward the end of the last cascade.
        shadow = mix(shadow, 1.0, smoothstep(0.85, 1.0, linearDepth));
      }
    #endif
    return mix(1.0, shadow, shadowStrength);
  }

  void main() {
    #ifdef SKY
      // The dome is opaque; the models over it (mountains, a moon, cloud
      // layers) keep their texture's alpha.
      vec4 sky = texture2D(map, vUv);
      #ifdef ALPHA_TEST
        if (sky.a < 0.5) discard;
      #endif
      #ifdef BLENDED
        gl_FragColor = sky;
      #else
        gl_FragColor = vec4(sky.rgb, 1.0);
      #endif
    #else
      #ifdef BUMP
        // The bump fetch's gradients, taken where every pixel still runs.
        // The fetch sits behind the BUMPMAPPING switch, and Direct3D's
        // compiler moves an implicit-gradient fetch out of a branch, so the
        // normal map would be read with bump mapping off.
        vec2 bumpDx = dFdx(vUv);
        vec2 bumpDy = dFdy(vUv);
      #endif
      #ifdef FAR
        // Reject the hidden near portion before any material texture fetches.
        if (vDepth < fade.x) discard;
      #endif
      #ifdef TERRAIN
        vec2 ground = vWorld.xz;
        #ifdef FAR
          vec4 texel = texture2D(map, (ground - farOrigin) / layerScales);
        #else
          #ifdef TERRAIN_LAYERS
            vec4 texel = paintedGround(ground);
          #else
            vec4 texel = mix(texture2D(map, ground / layerScales.x), texture2D(layer1Map, ground / layerScales.y), vColor.a);
          #endif
          float detail = 1.0 - smoothstep(30.0, 80.0, vDepth);
          // The detail texture is neutral past 80 m: no fetch there. Its
          // gradients come from outside the branch, for a defined mip level.
          vec2 groundDx = dFdx(ground);
          vec2 groundDy = dFdy(ground);
          texel.rgb *= detail > 0.0 ? mix(vec3(0.5), textureGrad(detailMap, ground / 2.0, groundDx / 2.0, groundDy / 2.0).rgb, detail) * 2.0 : vec3(1.0);
        #endif
      #else
        vec4 texel = texture2D(map, vUv);
      #endif
      #ifdef ALPHA_TEST
        // Coverage AA smooths texture silhouettes (fences/leaves), which
        // ordinary MSAA cannot touch after a binary shader discard.
        float coverage = 1.0;
        #ifdef FAR
          #define SAMPLES farAaSamples
        #else
          #define SAMPLES aaSamples
        #endif
        #ifndef BLENDED
        if (SAMPLES > 0.0) {
          float width = max(fwidth(texel.a), 0.0001);
          coverage = smoothstep(0.5 - width * 0.5, 0.5 + width * 0.5, texel.a);
          if (coverage <= 0.0) discard;
        } else
        #endif
        if (texel.a * 255.0 <= 127.0) discard;
      #endif
      #ifdef FAR
        float shadow = 1.0;
      #else
        float shadow = sunShadow();
      #endif

      vec3 tint = vec3(1.0);
      #ifdef TINT
        tint = vColor.rgb;
      #endif
      vec3 normal = normalize(vNormalW);
      float gloss = texel.a;
      vec3 point = vPoint;
      #if defined( DYNAMIC ) && !defined( NO_POINT ) && !defined( FAR )
        // Per pixel, the packed lights in world space (as in the vertex shader).
        point = vec3(0.0);
        for ( int i = 0; i < ZERO_LIGHTS; i ++ ) {
          if ( i >= zeroLightCount ) break;
          vec4 sphere = zeroLights[ 2 * i ];
          vec3 delta = sphere.xyz - vWorld;
          float range = sphere.w;
          float distanceSquared = dot(delta, delta);
          if (range <= 0.0 || distanceSquared < range * range) {
            float reach = range > 0.0 ? max(1.0 - distanceSquared / (range * range), 0.0) : 1.0;
            point += max(dot(normal, delta * inversesqrt(max(distanceSquared, 0.0001))), 0.0) * reach * zeroLights[ 2 * i + 1 ].rgb;
          }
        }
      #endif
      vec3 light;
      #if defined( UNLIT )
        light = tint;
      #elif defined( TERRAIN )
        #ifdef FAR
          light = vColor.rgb * 2.0;
          #ifdef TERRAIN_RELIGHT
            // The same slope shading as the near ground, without shadows.
            if (terrainShade > 0.5) light *= 0.5 + 0.5 * clamp(1.0 + (dot(normal, -sunDirection) / max(-sunDirection.y, 0.25) - 1.0) * 2.2, 0.0, 1.3);
          #endif
        #else
          light = min(vColor.rgb, bakedLit * mix(0.5, 1.0, shadow)) * 2.0;
          #ifdef TERRAIN_RELIGHT
            // Enhanced shading for ground baked flat (no sun in its vertex
            // colours, as on Hoth): the sun by the slope, scaled so open
            // flat ground keeps its baked value and shadow stays at half.
            // Slopes facing away fall toward the shadow level; slopes facing
            // the sun brighten a little. Shaded snow takes the sky's chill.
            if (terrainShade > 0.5) {
              // Twice the slope's own change, so gentle drifts still read.
              float slope = clamp(1.0 + (dot(normal, -sunDirection) / max(-sunDirection.y, 0.25) - 1.0) * 2.2, 0.0, 1.3);
              float sun = 0.5 + 0.5 * slope * shadow;
              light = min(vColor.rgb * max(sun, 1.0), bakedLit * sun) * 2.0;
              float snow = smoothstep(0.62, 0.85, min(texel.r, min(texel.g, texel.b)));
              light *= mix(vec3(1.0), vec3(0.9, 0.95, 1.05), snow * (1.0 - min(slope * shadow, 1.0)));
            }
          #endif
        #endif
      #elif defined( BAKED )
        light = clamp(vColor.rgb + vPoint, 0.0, 1.0);
      #else
        light = clamp((vAmbient + vSun * shadow + point) * tint, 0.0, 1.0);
        #ifdef BUMP
          if (bump > 0.5) {
            vec4 bumpTexel = textureGrad(bumpMap, vUv, bumpDx, bumpDy);
            vec3 n = bumpTexel.xyz * 2.0 - 1.0;
            normal = normalize(normalize(vTangent) * n.x + normalize(vBinormal) * n.y + normal * n.z);
            gloss = bumpTexel.a;
            // No sun at all on faces turned away from it.
            float lit = dot(vNormalW, -sunDirection) > 0.0 ? max(dot(normal, -sunDirection), 0.0) : 0.0;
            #ifdef DYNAMIC
              vec3 base = characterAmbient;
              base = mix(mix(base, vec3(0.0), ambientPull.x), vec3(1.0), ambientPull.y);
            #else
              vec3 base = ambient;
            #endif
            light = clamp(base + lit * sunColor * sunScale * shadow + point, 0.0, 1.0) * tint;
          }
        #endif
      #endif
      vec4 color = vec4(texel.rgb * light * brightness, texel.a);
      #ifdef DYNAMIC
        // Lamp illumination is per pixel and independent of baked terrain
        // darkening, so crossing a light pool doesn't tint an entire triangle.
        vec3 lamp = vec3(0.0);
        for (int i = 0; i < 4; i++) {
          // Unlit slots cost nothing: the count is a uniform.
          if (float(i) >= localLightCount) break;
          vec3 delta = localLightPositions[i].xyz - vWorld;
          float d2 = dot(delta, delta);
          float radius = max(localLightPositions[i].w, 0.001);
          float reach = max(1.0 - d2 / (radius * radius), 0.0);
          lamp += localLightColors[i] * reach * reach * max(dot(normal, delta * inversesqrt(max(d2, 0.0001))), 0.0);
        }
        color.rgb = texel.rgb * clamp(light * brightness + lamp * tint, 0.0, 1.0);
      #endif
      #ifdef SPECULAR
        // The sun's highlight, reflected toward the eye.
        vec3 toEye = normalize(cameraPosition - vWorld);
        float highlight = pow(max(dot(reflect(sunDirection, normal), toEye), 0.0), specular.x);
        color.rgb += highlight * specular.yzw * sunColor * sunScale * shadow * gloss;
      #endif

      #ifdef FAR
        // Linear fog by depth; additive surfaces fog toward black.
        float fog = clamp((vDepth - fogRange.x) / (fogRange.y - fogRange.x), 0.0, 1.0);
        #ifdef ADDITIVE
          color.rgb *= 1.0 - fog;
        #else
          color.rgb = mix(color.rgb, fogColor, fog);
        #endif
      #else
        float near = clamp((fade.y - vDepth) / max(fade.y - fade.x, 1.0), 0.0, 1.0);
        #ifdef BLENDED
          color.a *= near * near;
        #else
          // Most pixels are fully near: don't spend a far-scene texture read
          // on every fragment unless this pixel is actually in the dissolve.
          // An explicit level (the image has one) keeps the read inside the
          // branch on Direct3D, whose compiler hoists implicit-gradient ones.
          if (near < 1.0) color.rgb = mix(textureLod(farMap, gl_FragCoord.xy / resolution, 0.0).rgb, color.rgb, near);
          color.a = 1.0;
        #endif
      #endif
      #if defined( ALPHA_TEST ) && !defined( BLENDED )
        color.a = coverage;
      #endif
      gl_FragColor = color;
    #endif
  }
