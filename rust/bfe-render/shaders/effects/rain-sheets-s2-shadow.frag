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
#define SHADER_NAME 
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


  uniform sampler2D streaks, cover, sceneDepth;
  uniform float rainTime, speed, coverCell, coverSize, softParticles, strength;
  uniform vec2 wind, depthResolution, cameraRange;
  uniform vec3 color;
  // The map's ambient light (materials.js lighting), on a line of its own.
  uniform vec3 ambient;
  varying vec3 vWorld;
  varying vec2 vAround;
  varying float vLayer, vDepth;
  void main() {
    vec2 cell = mod(floor(vWorld.xz / coverCell), coverSize);
    if (vWorld.y < texture2D(cover, (cell + 0.5) / coverSize).r + 0.05) discard;
    // Farther sheets: finer, denser streaks moving more slowly on screen.
    float scale = vLayer < 0.5 ? 2.2 : vLayer < 1.5 ? 3.4 : 5.5;
    float slant = dot(wind, vec2(0.7, 0.7)) * 0.04;
    vec2 uv = vec2(vAround.x / scale + vWorld.y * slant, (vWorld.y + rainTime * speed) / (scale * 2.4));
    float a = texture2D(streaks, uv).a * 0.65 + texture2D(streaks, uv * vec2(1.7, 1.3) + vec2(0.31, rainTime * 0.21)).a * 0.35;
    float layer = vLayer < 0.5 ? 0.17 : vLayer < 1.5 ? 0.13 : 0.1;
    a *= layer * strength * smoothstep(0.0, 0.25, vAround.y) * (1.0 - smoothstep(0.75, 1.0, vAround.y));
    if (softParticles > 0.5) {
      float stored = texture2D(sceneDepth, gl_FragCoord.xy / depthResolution).r;
      float surface = cameraRange.x * cameraRange.y / (cameraRange.y - stored * (cameraRange.y - cameraRange.x));
      a *= clamp((surface - vDepth) / 1.5, 0.0, 1.0);
    }
    if (a < 0.003) discard;
    // Drops are lit by the whole sky around them and pass most of it on: a
    // little brighter than the scene's ambient light, much more in a flash.
    float sky = dot(ambient, vec3(0.3, 0.59, 0.11));
    gl_FragColor = vec4(color * clamp(0.45 + sky * 1.6, 0.0, 1.4), a);
  }
