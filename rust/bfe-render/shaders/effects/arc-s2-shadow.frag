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
#define SHADER_NAME arc-lightning
#define DOUBLE_SIDED
#define USE_SHADOWMAP
#define SHADOWMAP_TYPE_PCF_SOFT
uniform mat4 viewMatrix;
uniform vec3 cameraPosition;
uniform bool isOrthographic;
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


  uniform mat4 projectionMatrix;
  uniform vec2 fade;
  uniform vec2 eyeFade;
  uniform sampler2D sceneDepth;
  uniform float softParticles;
  uniform vec2 depthResolution;
  uniform vec2 cameraRange;
  varying vec2 vPlane;
  varying vec3 vA;
  varying vec3 vB;
  varying vec3 vNA;
  varying vec3 vNB;
  varying vec4 vColor;
  varying vec3 vSize;
  varying float vEnergy;
  varying float vCoreShare;
  void main() {
    vec3 ray = normalize(vec3(vPlane, -1.0));
    // Where two segments meet, the side of the plane halving their angle
    // that a pixel's ray passes the joint on decides which one draws it.
    if (dot(vNA, vNA) > 0.0 && dot(ray * dot(ray, vA) - vA, vNA) < 0.0) discard;
    if (dot(vNB, vNB) > 0.0 && dot(ray * dot(ray, vB) - vB, vNB) >= 0.0) discard;
    vec3 span = vB - vA;
    float rv = dot(ray, span), ra = dot(ray, vA), vv = dot(span, span);
    float den = vv - rv * rv;
    float s = den > 1e-9 ? clamp((rv * ra - dot(span, vA)) / den, 0.0, 1.0) : 0.0;
    vec3 closest = vA + span * s;
    float along = dot(ray, closest);
    if (along <= 0.0) discard;
    float d2 = max(dot(closest, closest) - along * along, 0.0);
    float d = sqrt(d2);
    float core = vSize.x, glow = vSize.y;
    float hot = (1.0 - smoothstep(core * 0.35, core * 1.25, d)) * vCoreShare;
    float sheath = exp(-d2 / (glow * glow) * 3.2);
    float halo = exp(-d / glow * 1.7) * 0.25;
    // Light adds up as energy and saturates per channel: the core goes white
    // while the sheath around it keeps the bolt's hue instead of clipping.
    vec3 energy = mix(vColor.rgb, vec3(1.0), 0.75) * hot * 2.4 * (0.4 + vSize.z) + vColor.rgb * (sheath * 1.7 + halo);
    vec3 color = 1.0 - exp(-energy * vColor.a * vEnergy);
    vec3 point = ray * along;
    float depth = -point.z;
    float near = clamp((fade.y - depth) / max(fade.y - fade.x, 1.0), 0.0, 1.0);
    // Nothing flares in the eye: in the world, what passes within half a
    // metre of it fades (not over the first-person arms, which are that
    // close).
    color *= near * near * (eyeFade.y > 0.0 ? smoothstep(eyeFade.x, eyeFade.y, depth) : 1.0);
    // Most of a capsule's quad is past its glow: let go before the scene's
    // depth is read (the soft edge only dims it further).
    if (max(color.r, max(color.g, color.b)) < 0.003) discard;
    if (softParticles > 0.5) {
      float stored = texture2D(sceneDepth, gl_FragCoord.xy / depthResolution).x;
      float surface = cameraRange.x * cameraRange.y / (cameraRange.y - stored * (cameraRange.y - cameraRange.x));
      color *= smoothstep(0.0, max(glow * 0.6, 0.04), surface - depth);
    }
    if (max(color.r, max(color.g, color.b)) < 0.003) discard;
    // Only the projection's z and w rows make the depth.
    vec4 hp = vec4(point, 1.0);
    float clipZ = dot(vec4(projectionMatrix[0].z, projectionMatrix[1].z, projectionMatrix[2].z, projectionMatrix[3].z), hp);
    float clipW = dot(vec4(projectionMatrix[0].w, projectionMatrix[1].w, projectionMatrix[2].w, projectionMatrix[3].w), hp);
    gl_FragDepth = clamp(clipZ / clipW * 0.5 + 0.5, 0.0, 1.0);
    // Premultiplied: the light adds, and the sheath also covers part of what
    // is behind it, so the bolt keeps its hue in front of bright ground or sky.
    gl_FragColor = vec4(color, max(color.r, max(color.g, color.b)) * 0.45);
  }
