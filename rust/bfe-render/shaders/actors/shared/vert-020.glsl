void getDirectionalLightInfo( const in DirectionalLight directionalLight, out IncidentLight light ) {
light.color = directionalLight.color;
light.direction = directionalLight.direction;
light.visible = true;
}
#endif
#if 0 > 0
struct PointLight {
vec3 position;
vec3 color;
float distance;
float decay;
};
uniform PointLight pointLights[ 0 ];
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
uniform vec3 sunDirection;
uniform vec3 sunColor;
uniform vec3 ambient;
uniform vec3 ambientTop;
uniform vec3 ambientBottom;
uniform float sunScale;
uniform vec2 ambientPull;
uniform float time;
uniform vec2 scroll;
#if !defined( NO_POINT ) && !defined( DYNAMIC ) && !defined( FAR )
uniform vec4 zeroLights[ZERO_LIGHTS * 2];
uniform int zeroLightCount;
#endif
#ifdef HAS_COLOR
attribute vec4 color;
#endif
#ifdef BUMP
attribute vec3 tangent;
attribute vec3 binormal;
varying vec3 vTangent;
varying vec3 vBinormal;
#endif
#ifdef TERRAIN_LAYERS
attribute vec4 weights0;
attribute vec4 weights1;
varying vec4 vWeights0;
varying vec4 vWeights1;
#endif
varying vec2 vUv;
varying vec4 vColor;
varying vec3 vAmbient;
varying vec3 vSun;
varying vec3 vPoint;
varying vec3 vNormalW;
varying vec3 vWorld;
varying float vDepth;
void main() {
#ifdef TERRAIN_LAYERS
vWeights0 = weights0;
vWeights1 = weights1;
#endif
vec3 objectNormal = vec3( normal );
#ifdef USE_TANGENT
vec3 objectTangent = vec3( tangent.xyz );
#endif
#ifdef WORLD_SPACE
// The map's own meshes (world.js): vertices already in the world and
// a model matrix of identity, so the model-view matrix is the view
// matrix itself and the model and normal matrices are skipped. Older
// GPUs read every uniform a vertex uses from memory, a read each row.
vec3 transformed = vec3( position );
#ifdef USE_ALPHAHASH
vPosition = vec3( position );
