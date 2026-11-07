#if defined( USE_ENVMAP ) || defined( DISTANCE ) || defined ( USE_SHADOWMAP ) || defined ( USE_TRANSMISSION ) || 0 > 0
vec4 worldPosition = vec4( transformed, 1.0 );
#ifdef USE_BATCHING
worldPosition = batchingMatrix * worldPosition;
#endif
#ifdef USE_INSTANCING
worldPosition = instanceMatrix * worldPosition;
#endif
worldPosition = modelMatrix * worldPosition;
#endif
#else
#ifdef USE_SKINNING
mat4 boneMatX = getBoneMatrix( skinIndex.x );
mat4 boneMatY = getBoneMatrix( skinIndex.y );
mat4 boneMatZ = getBoneMatrix( skinIndex.z );
mat4 boneMatW = getBoneMatrix( skinIndex.w );
#endif
#ifdef USE_SKINNING
mat4 skinMatrix = mat4( 0.0 );
skinMatrix += skinWeight.x * boneMatX;
skinMatrix += skinWeight.y * boneMatY;
skinMatrix += skinWeight.z * boneMatZ;
skinMatrix += skinWeight.w * boneMatW;
skinMatrix = bindMatrixInverse * skinMatrix * bindMatrix;
objectNormal = vec4( skinMatrix * vec4( objectNormal, 0.0 ) ).xyz;
#ifdef USE_TANGENT
objectTangent = vec4( skinMatrix * vec4( objectTangent, 0.0 ) ).xyz;
#endif
#endif
vec3 transformedNormal = objectNormal;
#ifdef USE_TANGENT
vec3 transformedTangent = objectTangent;
#endif
#ifdef USE_BATCHING
mat3 bm = mat3( batchingMatrix );
transformedNormal /= vec3( dot( bm[ 0 ], bm[ 0 ] ), dot( bm[ 1 ], bm[ 1 ] ), dot( bm[ 2 ], bm[ 2 ] ) );
transformedNormal = bm * transformedNormal;
#ifdef USE_TANGENT
transformedTangent = bm * transformedTangent;
#endif
#endif
#ifdef USE_INSTANCING
mat3 im = mat3( instanceMatrix );
transformedNormal /= vec3( dot( im[ 0 ], im[ 0 ] ), dot( im[ 1 ], im[ 1 ] ), dot( im[ 2 ], im[ 2 ] ) );
transformedNormal = im * transformedNormal;
#ifdef USE_TANGENT
transformedTangent = im * transformedTangent;
#endif
#endif
transformedNormal = normalMatrix * transformedNormal;
#ifdef FLIP_SIDED
transformedNormal = - transformedNormal;
#endif
#ifdef USE_TANGENT
transformedTangent = ( modelViewMatrix * vec4( transformedTangent, 0.0 ) ).xyz;
#ifdef FLIP_SIDED
transformedTangent = - transformedTangent;
#endif
#endif
vec3 transformed = vec3( position );
#ifdef USE_ALPHAHASH
vPosition = vec3( position );
#endif
#ifdef USE_SKINNING
vec4 skinVertex = bindMatrix * vec4( transformed, 1.0 );
vec4 skinned = vec4( 0.0 );
skinned += boneMatX * skinVertex * skinWeight.x;
skinned += boneMatY * skinVertex * skinWeight.y;
skinned += boneMatZ * skinVertex * skinWeight.z;
skinned += boneMatW * skinVertex * skinWeight.w;
transformed = ( bindMatrixInverse * skinned ).xyz;
#endif
vec4 mvPosition = vec4( transformed, 1.0 );
#ifdef USE_BATCHING
mvPosition = batchingMatrix * mvPosition;
#endif
#ifdef USE_INSTANCING
mvPosition = instanceMatrix * mvPosition;
#endif
mvPosition = modelViewMatrix * mvPosition;
gl_Position = projectionMatrix * mvPosition;
#if defined( USE_ENVMAP ) || defined( DISTANCE ) || defined ( USE_SHADOWMAP ) || defined ( USE_TRANSMISSION ) || 0 > 0
vec4 worldPosition = vec4( transformed, 1.0 );
#ifdef USE_BATCHING
worldPosition = batchingMatrix * worldPosition;
#endif
#ifdef USE_INSTANCING
worldPosition = instanceMatrix * worldPosition;
#endif
worldPosition = modelMatrix * worldPosition;
#endif
#endif
// Scrolling materials move at -speed * t, in repeats per second.
vUv = uv;
#ifdef SCROLL
vUv -= scroll * time;
#endif
#ifdef HAS_COLOR
vColor = color;
#else
vColor = vec4(1.0);
#endif
#ifdef WORLD_SPACE
vec3 n = normalize(objectNormal);
vNormalW = n;
vWorld = transformed;
#else
// transformedNormal includes skinning and the inverse-transpose normal
// matrix. A plain modelMatrix multiplication breaks nonuniform scales.
vec3 n = inverseTransformDirection(transformedNormal, viewMatrix);
vNormalW = n;
vWorld = (modelMatrix * vec4(transformed, 1.0)).xyz;
#endif
vDepth = -mvPosition.z;
#ifdef DYNAMIC
vAmbient = mix(ambientTop, ambientBottom, 0.5 * (1.0 - n.y));
// A light region pulls a dynamic object's ambient toward black or white.
vAmbient = mix(mix(vAmbient, vec3(0.0), ambientPull.x), vec3(1.0), ambientPull.y);
#else
vAmbient = ambient;
#endif
vSun = max(dot(n, -sunDirection), 0.0) * sunColor * sunScale;
#ifdef BUMP
vec3 tangentObject = tangent;
vec3 binormalObject = binormal;
#ifdef USE_SKINNING
tangentObject = mat3(skinMatrix) * tangentObject;
binormalObject = mat3(skinMatrix) * binormalObject;
#endif
#ifdef WORLD_SPACE
vTangent = tangentObject;
vBinormal = binormalObject;
#else
vTangent = mat3(modelMatrix) * tangentObject;
vBinormal = mat3(modelMatrix) * binormalObject;
#endif
#endif
// Point lights: max(0, N.toLight) * (1 - d^2 / r^2) * colour, for the
// lights that are on and within reach of what is drawn (point-lights.js),
// in world space. Past its range a light adds nothing: its colour isn't
// even read.
vPoint = vec3(0.0);
#if !defined( NO_POINT ) && !defined( DYNAMIC ) && !defined( FAR )
for ( int i = 0; i < ZERO_LIGHTS; i ++ ) {
if ( i >= zeroLightCount ) break;
vec4 sphere = zeroLights[ 2 * i ];
vec3 toLight = sphere.xyz - vWorld;
float range = sphere.w;
float distanceSquared = dot(toLight, toLight);
if (range <= 0.0 || distanceSquared < range * range) {
float reach = range > 0.0 ? max(1.0 - distanceSquared / (range * range), 0.0) : 1.0;
vPoint += max(dot(n, normalize(toLight)), 0.0) * reach * zeroLights[ 2 * i + 1 ].rgb;
}
}
#endif
}
