#if defined( USE_SHADOWMAP ) && 2 > 0
uniform highp sampler2DShadow cascadeMaps[ 2 ];
// The map coordinates come from the world position here, for the one or
// two cascades a pixel reads, instead of four vec4 varyings a vertex.
uniform mat4 directionalShadowMatrix[ 2 ];
uniform mat3 cascadePlanes[ 2 ];
