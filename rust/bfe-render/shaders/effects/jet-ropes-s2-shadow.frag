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
  varying vec3 vWorld, vSide, vAlong, vFlow;
  varying vec4 vRope;
  varying float vAcross, vHalf, vSpeed, vFall, vLit, vCover, vDepth, vLevel, vPixel;
  const float TAU = 6.2831853;
  
  float hash1(float p) { p = fract(p * 0.1031); p *= p + 33.33; p *= p + p; return fract(p); }
  float noise1(float x) { float i = floor(x), f = fract(x); f = f * f * (3.0 - 2.0 * f); return mix(hash1(i), hash1(i + 1.0), f); }
  vec4 hash4(float p) {
    vec4 q = fract(vec4(p) * vec4(.1031, .1030, .0973, .1099));
    q += dot(q, q.wzxy + 33.33);
    return fract((q.xxyz + q.yzzw) * q.zywx);
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
    float f = vFlow.x, j = vFlow.y, salt = vFlow.z + j * 1.7, fall = vFall, px = vPixel;
    // It carries the same water as it speeds up, so it thins; it swells and
    // necks as its water passes, and wavers.
    float r = vRope.x * sqrt(1.5 / max(vSpeed, 1.5)) * (0.8 + 0.4 * noise1(f * 4.0 + salt));
    float across = vAcross * vHalf + 0.5 * r * (noise1(f * 17.0 - jetTime * 1.7 + salt * 5.3) - 0.5);
    // Past its break point it beads and parts into drops (Plateau-Rayleigh):
    // a bead every few diameters, each the water that left the crown in that
    // moment, so the drops ride with it; some beads come to nothing.
    float A = smoothstep(vRope.z, vRope.z + 0.25, fall);
    float cellF = 0.75 * vRope.x, w = f / cellF + hash1(j * 3.3 + salt);
    float cell = floor(w), u = w - cell - 0.5, along = u * cellF * vSpeed;
    float rb = min(1.9 * r * (0.6 + 0.8 * hash1(cell * 1.37 + salt)), 0.009) * smoothstep(0.0, 0.35, A) * step(0.15, hash1(cell * 2.9 + salt * 1.3));
    // A drop, smeared over a sixtieth of a second.
    float halfLen = rb + vSpeed / 120.0;
    float stream = r * (1.0 + 0.7 * A * cos(TAU * u)) * (1.0 - smoothstep(0.55, 0.95, A));
    // How much of this pixel each covers, for the pixel's own width.
    float inStream = clamp((min(across + 0.5 * px, stream) - max(across - 0.5 * px, -stream)) / px, 0.0, 1.0);
    float e = length(vec2(along / max(halfLen, 1e-5), across / max(rb, 1e-5)));
    float inDrop = rb > 1e-5 ? (1.0 - smoothstep(1.0 - 0.5 * px / rb, 1.0 + 0.5 * px / rb, e)) * min(1.0, 2.0 * rb / px) : 0.0;
    float cover = max(inStream, inDrop) * vCover * vRope.y;
    // Faded softly into the pool and where it passes behind the bowl.
    cover *= smoothstep(vLevel - 0.05, vLevel + 0.25, vWorld.y) * softly(vDepth, 0.3);
    if (cover < 0.002) discard;
    // Round across, like a rod of water; a drop is round all over.
    bool drop = inDrop > inStream;
    float nx = clamp(across / max(drop ? rb : stream, 1e-5), -1.0, 1.0), ny = drop ? clamp(along / halfLen, -1.0, 1.0) : 0.0;
    vec3 V = normalize(cameraPosition - vWorld);
    vec3 N = normalize(vSide * nx + vAlong * ny * 0.6 + V * sqrt(max(1.0 - nx * nx - ny * ny * 0.36, 0.04)));
    // White where air churns in it, in patches, and all through at the
    // crown; its drops are clear. Far off, bubbles and glints too small to
    // draw average into white.
    float aerated = clamp(vRope.w * (0.55 + 0.8 * noise1(f * 6.0 + salt * 2.1)) + 0.7 * (1.0 - smoothstep(0.0, 0.2, fall)), 0.0, 1.0);
    if (drop) aerated *= 0.35;
    aerated = max(aerated, 0.55 * smoothstep(0.015, 0.06, px));
    // Ripples riding with the water catch the sun.
    vec3 ripple = (vSide * (noise1(f * 140.0 + across / max(r, 1e-3) + salt) - 0.5) + vAlong * (noise1(f * 97.0 + salt * 3.0) - 0.5)) * 2.6;
    gl_FragColor = water(N, V, vLit, aerated, sqrt(max(1.0 - nx * nx, 0.0)), cover, 0.04, 2.0, ripple, 0.0);
    // Glints: here and there a facet of the tumbling surface turns the sun to
    // the eye for a moment. Each is a point a pixel and a half across,
    // however far, more of them looking toward the sun.
    vec2 g = vec2(f / 0.035, (vAcross + 1.0) * 2.0), gi = floor(g), gp = (fract(g) - 0.5) * vec2(0.035 * vSpeed, 0.5 * vHalf) / px;
    float chance = (0.04 + 0.08 * forward(V)) * vLit * vLit;
    float glint = step(1.0 - chance, hash1(gi.x * 7.13 + gi.y * 1.7 + floor(jetTime * 14.0 + hash1(gi.x) * 3.0) * 0.37 + salt));
    gl_FragColor.rgb += sunLight() * 2.5 * glint * (1.0 - smoothstep(0.4, 1.2, length(gp))) * min(cover * 3.0, 1.0);
  }
