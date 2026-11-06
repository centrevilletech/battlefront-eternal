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
#define USE_SHADOWMAP
#define SHADOWMAP_TYPE_PCF_SOFT
#define PREMULTIPLIED_ALPHA
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


  uniform sampler2D sceneDepth;
  uniform vec2 depthResolution, cameraRange;
  uniform vec2 fade;
  uniform float hasDepth, jetTime;
  varying vec3 vWorld, vNormal, vFlow;
  varying float vY, vAbove, vAlpha, vLit, vPart, vDepth;
  const float TOP = 6.40;
  
  float hash3(vec3 p) { p = fract(p * 0.1031); p += dot(p, p.zyx + 31.32); return fract((p.x + p.y) * p.z); }
  float noise3(vec3 p) {
    vec3 i = floor(p), f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    return mix(mix(mix(hash3(i), hash3(i + vec3(1.0, 0.0, 0.0)), f.x), mix(hash3(i + vec3(0.0, 1.0, 0.0)), hash3(i + vec3(1.0, 1.0, 0.0)), f.x), f.y),
      mix(mix(hash3(i + vec3(0.0, 0.0, 1.0)), hash3(i + vec3(1.0, 0.0, 1.0)), f.x), mix(hash3(i + vec3(0.0, 1.0, 1.0)), hash3(i + 1.0), f.x), f.y), f.z);
  }

  
  uniform vec3 sunDirection;
  uniform vec3 sunColor;
  uniform vec3 ambient;
  uniform vec3 ambientTop;
  uniform vec3 ambientBottom;
  // The sun, a little less yellow than the map's (tuned for its lightmaps).
  vec3 sunLight() { return mix(sunColor, vec3(dot(sunColor, vec3(0.3333))), 0.35); }
  vec3 skyAbove() { return ambientTop * 1.3 + vec3(0.1, 0.13, 0.27); }
  // Light from the sky, and from the warm square below, on a face toward N.
  vec3 skyFill(vec3 N) { return mix(mix(ambient, ambientTop, 0.5) * 1.4 + vec3(0.06, 0.08, 0.14), ambientBottom * 0.85, 0.3 * max(-N.y, 0.0)); }
  // What a mirror shows: sky above, the square's walls toward the horizon, its floor below.
  vec3 skyReflect(vec3 R) {
    vec3 above = skyAbove(), around = mix(above, ambientBottom, 0.3), below = ambientBottom * 0.6;
    return R.y > 0.0 ? mix(around, above, smoothstep(0.0, 0.6, R.y)) : mix(around, below, smoothstep(0.0, 0.4, -R.y));
  }
  // Sunlight scattered toward the eye by spray and thin water, strongest
  // looking toward the sun (Henyey-Greenstein, g = 0.45; 1 is even
  // scattering), held below the sun's own glare.
  float forward(vec3 V) {
    float c = dot(-V, -sunDirection);
    return min(0.7975 / pow(1.2025 - 0.9 * c, 1.5), 2.5);
  }
  // Bright water rolls off toward a light grey-white: only glints, drawn
  // over this, reach white.
  vec3 soften(vec3 c) {
    float m = max(c.r, max(c.g, c.b));
    return m > 0.8 ? c * ((0.8 + 0.15 * (1.0 - exp(-(m - 0.8) / 0.15))) / m) : c;
  }
  // Spray, lit as the shape it hangs in (normal N): the sun on that side, the
  // sky in its shade, and the sun scattered through it toward the eye.
  vec3 sprayLight(vec3 N, vec3 V, float lit) {
    float facing = clamp(dot(N, -sunDirection) * 0.6 + 0.4, 0.0, 1.0);
    return soften(skyFill(N) * 0.9 + sunLight() * lit * (0.65 * facing * facing + 0.6 * forward(V)));
  }

  
  uniform sampler2D sceneColor;
  uniform vec2 sceneResolution;
  uniform float hasScene;
  // lit: sunlight reaching here through the plume; aerated: 0 glassy, 1
  // white; thick: the water's depth through here, 0 at its edge; bend: how
  // far it displaces the scene behind; blur: the mipmap level that scene is
  // read from; ripple: a small tilt of the surface; spark: a facet of it
  // turning the sun to the eye right here.
  vec4 water(vec3 N, vec3 V, float lit, float aerated, float thick, float alpha, float bend, float blur, vec3 ripple, float spark) {
    vec3 L = -sunDirection, sun = sunLight();
    float NV = max(dot(N, V), 0.0), NL = dot(N, L), F = 0.02 + 0.98 * pow(1.0 - NV, 5.0), phase = forward(V);
    // Like cloud: bright toward the sun, the sky in its shade, and the sun
    // carried on through its thin parts toward the eye.
    vec3 scatter = skyFill(N) + sun * lit * (0.62 * max(NL, 0.0) + 0.14 * (NL * 0.5 + 0.5) + 0.8 * phase * (1.0 - 0.5 * thick));
    // Clear: the sky and square reflected, strongest at grazing angles; the
    // scene behind, bent, blurred and tinted; backlit, the sun bent through
    // its sides to the eye.
    vec3 clear = skyReflect(reflect(-V, N)) * F + sun * lit * phase * 0.45 * pow(1.0 - NV, 1.5);
    // (Mostly white water hides what lies behind it: skip reading it.)
    if (hasScene > 0.5 && aerated < 0.85) {
      vec2 shift = (viewMatrix * vec4(N, 0.0)).xy * bend;
      vec3 behind = textureLod(sceneColor, clamp(gl_FragCoord.xy / sceneResolution + shift, 0.002, 0.998), blur).rgb;
      clear += (behind * vec3(0.84, 0.95, 0.97) + vec3(0.0, 0.015, 0.025)) * (1.0 - F);
    } else if (hasScene > 0.5) clear += skyFill(N) * 0.6 * (1.0 - F);
    // Aerated water is translucent where it thins; layers of it come to its
    // own colour, never past it. Glints go on top in full: they are small,
    // and should reach white.
    float clearOpacity = hasScene > 0.5 ? 1.0 : min(F + 0.12, 1.0), whiteOpacity = 0.88 + 0.12 * thick;
    vec3 H = normalize(L + V), R = normalize(N + ripple);
    float spec = pow(max(dot(N, H), 0.0), 220.0) * 1.5 + pow(max(dot(R, H), 0.0), 500.0) * 5.0
      + spark * 4.0 * smoothstep(0.2, 0.8, dot(R, H));
    vec3 color = mix(soften(clear), soften(scatter) * whiteOpacity, aerated) + sun * spec * lit * (1.0 - 0.6 * aerated);
    return vec4(color * alpha, alpha * mix(clearOpacity, whiteOpacity, aerated));
  }

  
  // Frame pixels per pixel drawn: 2 for the half-size soft spray.
  uniform vec2 fragScale;
  // Soft where it meets the scene, faded with the near scene's far fade and
  // right in front of the eye.
  float softly(float depth, float reach) {
    float a = clamp((fade.y - depth) / max(fade.y - fade.x, 1.0), 0.0, 1.0) * smoothstep(0.4, 1.2, depth);
    if (hasDepth < 0.5) return a;
    float z = texture2D(sceneDepth, gl_FragCoord.xy * fragScale / depthResolution).r;
    float surface = cameraRange.x * cameraRange.y / (cameraRange.y - z * (cameraRange.y - cameraRange.x));
    return a * smoothstep(0.0, reach, surface - depth);
  }

  void main() {
    vec3 V = normalize(cameraPosition - vWorld), axis = normalize(vNormal);
    float facing = abs(dot(axis, V)), aerated, cover;
    vec3 N;
    if (vPart < 0.5) {
      // Streaks: fine around the jet, long along it; one layer rides with
      // the water, one slips along it, and fine bubbles churn through both.
      float a = noise3(vec3(vFlow.xy * 6.0, vFlow.z * 9.0)), b = noise3(vec3(vFlow.xy * 17.0 + 3.0, vFlow.z * 5.0 - jetTime * 1.3));
      float c = noise3(vec3(vFlow.xy * 34.0 + 7.0, vFlow.z * 23.0));
      vec3 tangent = normalize(cross(vec3(0.0, 1.0, 0.0), axis) + vec3(1e-4, 0.0, 0.0));
      N = normalize(axis + tangent * (b - 0.5) * 0.9 + vec3(0.0, c - 0.5, 0.0) * 0.7);
      // Glassy where it leaves the water; above, churning white water.
      aerated = smoothstep(0.15, 0.9, vAbove + (a - 0.5) * 0.5) * (0.85 + 0.15 * c);
      cover = smoothstep(0.02, 0.3, facing + (a - 0.5) * 0.4);
      cover *= mix(1.0, smoothstep(0.15, 0.5, 0.6 * a + 0.4 * b), 0.5 * aerated);
      cover *= (1.0 - smoothstep(0.6 * TOP, TOP, vY + (b - 0.5) * 1.2)) * 0.95;
    } else {
      // Foam boiling on the dome, torn open to the water beneath, thinning
      // to nothing at its edge.
      float a = noise3(vFlow), b = noise3(vFlow * 2.3 + 7.0), c = noise3(vFlow * 5.1 + 3.0);
      N = normalize(axis + vec3(a - 0.5, 0.0, b - 0.5) * 1.2 + vec3(c - 0.5, 0.0, a - 0.5) * 0.6);
      aerated = 0.55 + 0.45 * smoothstep(0.3, 0.7, b * 0.6 + c * 0.4);
      cover = smoothstep(0.0, 0.5, vY) * mix(0.0, 0.7, smoothstep(0.45, 0.8, a * 0.5 + b * 0.3 + c * 0.2));
    }
    cover *= vAlpha * softly(vDepth, 0.15);
    if (cover < 0.002) discard;
    // The jet is the brightest water in the fountain: little of the
    // umbrella's shade reaches it.
    gl_FragColor = water(N, V, max(vLit, 0.8), aerated, facing, cover, 0.06, 1.2, vec3(0.0), 0.0);
  }
