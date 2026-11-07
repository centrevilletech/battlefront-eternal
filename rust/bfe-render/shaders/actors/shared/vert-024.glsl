vPosition = vec3( position );
#endif
transformed = ( bindMatrixInverse * ( boneMatX * vec4( transformed, 1.0 ) * skinWeight.x ) ).xyz;
vec4 mvPosition = vec4( transformed, 1.0 );
#ifdef USE_BATCHING
mvPosition = batchingMatrix * mvPosition;
#endif
#ifdef USE_INSTANCING
mvPosition = instanceMatrix * mvPosition;
