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
