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
  uniform float time;
  uniform sampler2D sceneDepth;
  uniform float softParticles;
  uniform vec2 depthResolution;
  uniform vec2 cameraRange;
  varying vec2 vPlane;
  varying vec3 vHead;
  varying vec3 vTail;
  varying vec4 vColor;
  varying vec4 vSize;
  varying vec4 vMisc;
  varying float vEnergy;
  varying float vCoreShare;
  
  float hash12(vec2 p) {
    vec3 p3 = fract(vec3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
  }
  float vnoise(vec2 p) {
    vec2 i = floor(p), f = fract(p);
    vec2 u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash12(i), hash12(i + vec2(1.0, 0.0)), u.x),
               mix(hash12(i + vec2(0.0, 1.0)), hash12(i + vec2(1.0, 1.0)), u.x), u.y);
  }
  float fbm(vec2 p) {
    return vnoise(p) * 0.55 + vnoise(p * 2.13 + 7.1) * 0.3 + vnoise(p * 4.37 + 3.7) * 0.15;
  }

  void main() {
    vec3 ray = normalize(vec3(vPlane, -1.0));
    vec3 span = vTail - vHead;
    float rv = dot(ray, span), rh = dot(ray, vHead), vv = dot(span, span);
    float den = vv - rv * rv;
    float s = den > 1e-6 ? clamp((rv * rh - dot(span, vHead)) / den, 0.0, 1.0) : 0.0;
    vec3 closest = vHead + span * s;
    float along = dot(ray, closest);
    if (along <= 0.0) discard;
    float d2 = max(dot(closest, closest) - along * along, 0.0);
    float d = sqrt(d2);
    float len = sqrt(vv);
    // Metres from the muzzle (the tail end) at this pixel.
    float m = vMisc.z + (1.0 - s) * len;
    float age = vMisc.x;
    float kind = vSize.z;
    float seed = vSize.w;
    vec3 rgb = vColor.rgb;
    // A red-dominant bolt (the CIS's) is drawn as the films draw droid
    // bolts: a thin white-hot core inside a wide, bright red glow. Spread
    // across the whole core, the white's green and blue turned it pink.
    float redness = clamp((rgb.r - max(rgb.g, rgb.b)) * 1.2, 0.0, 1.0);
    float whiten = mix(0.45, 0.85, redness);
    float thinCore = mix(1.0, 0.45, redness);
    float redGlow = mix(1.0, 1.7, redness);
    vec3 point = ray * along;
    float depth = -point.z;
    float near = clamp((fade.y - depth) / max(fade.y - fade.x, 1.0), 0.0, 1.0);
    // Nothing flares right in front of the eye (the shooter's own view down
    // the shot), but the puff at his own muzzle (vMisc.w).
    float eye = vMisc.w > 0.5 ? smoothstep(0.08, 0.3, depth) : smoothstep(0.3, 2.5, depth);
    float keep = near * near * eye;
    vec3 energy = vec3(0.0);
    float cover = 0.0;
    if (kind < 0.5) {
      // A tracer line: an even glow along it.
      float core = vSize.x * thinCore, glow = vSize.y;
      float axial = vv > 1e-8 ? abs(rv) / len : 1.0;
      float hot = (1.0 - smoothstep(core * 0.5, core * 1.15, d)) * vCoreShare;
      float sheath = exp(-d2 / (glow * glow) * 2.6) * (1.0 + 0.4 * axial);
      float halo = exp(-d / glow * 2.0) * 0.32;
      float pulse = 1.0 + 0.35 * exp(-age * 2.5) * sin(m * 0.8 - age * 70.0 + seed * 6.28);
      float shimmer = 0.9 + 0.1 * sin(time * 83.0 + seed * 40.0 + m * 0.3);
      energy = (mix(rgb, vec3(1.0), whiten) * hot * 2.2 + rgb * (sheath * 2.0 + halo) * redGlow) * pulse * shimmer;
      energy *= vColor.a * vEnergy;
    } else if (kind < 1.5) {
      // Vapour: a soft tube that tears into wisps as it ages.
      float radius = vSize.y * 0.5;
      float body = exp(-d2 / (radius * radius) * 1.6);
      // The noise below is the costliest part of the look: where even its
      // densest (1.3 x body) would end under the discard below, the pixel
      // is let go first.
      if (body * 1.3 * keep * (vColor.a * 0.9 + vMisc.y * 0.6) < 0.002) discard;
      float tear = fbm(vec2(m * 0.42 + seed * 31.0, age * 0.55));
      float wisp = fbm(vec2(m * 1.6 - seed * 7.0, d / radius * 1.7 - age * 1.2));
      float density = body * smoothstep(0.18, 0.72, tear + 0.42 - age * 0.22) * (0.55 + 0.75 * wisp);
      cover = clamp(density * vMisc.y, 0.0, 1.0);
      // Fresh vapour still glows with the shot's colour.
      energy = rgb * density * vColor.a * 0.9;
    } else {
      // A spark: a short tapering streak.
      float core = vSize.x * thinCore * mix(1.0, 0.25, s), glow = vSize.y * mix(1.0, 0.5, s) * mix(1.0, 1.25, redness);
      float hot = (1.0 - smoothstep(core * 0.5, core * 1.2, d)) * vCoreShare;
      float sheath = exp(-d2 / (glow * glow) * 2.4);
      energy = (mix(rgb, vec3(1.0), whiten) * hot * 2.0 + rgb * sheath * 2.0 * redGlow) * vColor.a * mix(1.0, 0.25, s) * vEnergy;
    }
    // A bolt or tracer comes out of the gun: it fades in over its first
    // metres (vMisc.y), so its world-fixed start never shows as an edge
    // beside a gun that has since kicked and moved on.
    if (kind < 0.5 || kind > 1.5) energy *= smoothstep(0.0, max(vMisc.y, 1e-3), m);
    vec3 light = (1.0 - exp(-energy)) * keep;
    cover *= keep;
    // Vapour covers what is behind it in a pale haze; light adds.
    vec3 haze = mix(vec3(0.62, 0.66, 0.72), rgb, 0.18) * cover * 0.55;
    vec3 color = light + haze;
    // It also covers a little of the ground behind its glow, so the red
    // holds over bright sand without going dark.
    float shade = kind > 0.5 && kind < 1.5 ? 0.5 : mix(0.5, 0.62, redness);
    float alpha = max(max(light.r, max(light.g, light.b)) * shade, cover * 0.6);
    // Faint already, it is let go before the scene's depth is read (the
    // soft edge below only dims it further).
    if (max(color.r, max(color.g, color.b)) < 0.002 && alpha < 0.002) discard;
    if (softParticles > 0.5) {
      float stored = texture2D(sceneDepth, gl_FragCoord.xy / depthResolution).x;
      float surface = cameraRange.x * cameraRange.y / (cameraRange.y - stored * (cameraRange.y - cameraRange.x));
      float soft = smoothstep(0.0, max(vSize.y * 0.8, 0.05), surface - depth);
      color *= soft;
      alpha *= soft;
      if (max(color.r, max(color.g, color.b)) < 0.002 && alpha < 0.002) discard;
    }
    vec4 clip = projectionMatrix * vec4(point, 1.0);
    gl_FragDepth = clamp(clip.z / clip.w * 0.5 + 0.5, 0.0, 1.0);
    gl_FragColor = vec4(color, alpha);
  }
