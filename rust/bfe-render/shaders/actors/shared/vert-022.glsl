#endif
vec4 mvPosition = viewMatrix * vec4( transformed, 1.0 );
gl_Position = projectionMatrix * mvPosition;
#elif defined( USE_SKINNING ) && defined( RIGID_SKIN )
// One bone a vertex at full weight and a bind matrix of identity (the
// 2004 game's soldiers, actor.js). three's blend of four bones reduces
// to this term for term: the other weights are zero and multiplying by
// the identity bind matrix changes nothing. One bone's matrix is read
// instead of four (each four texel reads) and two matrix products go.
mat4 boneMatX = getBoneMatrix( skinIndex.x );
mat4 skinMatrix = bindMatrixInverse * ( skinWeight.x * boneMatX );
objectNormal = vec4( skinMatrix * vec4( objectNormal, 0.0 ) ).xyz;
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
