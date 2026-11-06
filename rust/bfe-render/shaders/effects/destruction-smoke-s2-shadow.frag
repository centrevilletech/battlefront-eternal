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
#define SHADER_NAME destruction-smoke
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


  uniform vec3 sunColor;
  uniform vec3 ambient;
  uniform vec3 fogColor;
  uniform vec2 fogRange;
  varying vec2 vUv;
  varying vec4 vState;
  varying vec4 vLook;
  varying vec3 vSun;
  varying float vDepth;
  
  float dHash(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
  float dNoise(vec2 p) {
    vec2 i = floor(p), f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    return mix(mix(dHash(i), dHash(i + vec2(1.0, 0.0)), f.x), mix(dHash(i + vec2(0.0, 1.0)), dHash(i + vec2(1.0, 1.0)), f.x), f.y);
  }
  // Value noise and its slope (x: value, yz: gradient).
  vec3 dNoiseD(vec2 x) {
    vec2 i = floor(x), f = fract(x);
    vec2 u = f * f * (3.0 - 2.0 * f), du = 6.0 * f * (1.0 - f);
    float a = dHash(i), b = dHash(i + vec2(1.0, 0.0)), c = dHash(i + vec2(0.0, 1.0)), d = dHash(i + vec2(1.0, 1.0));
    float k = a - b - c + d;
    return vec3(a + (b - a) * u.x + (c - a) * u.y + k * u.x * u.y, du * (vec2(b - a, c - a) + k * u.yx));
  }
  // Four octaves of it, the slopes summed alongside.
  vec3 dFbmD(vec2 p) {
    vec3 sum = vec3(0.0);
    float amplitude = 0.5, frequency = 1.0;
    for (int i = 0; i < 4; i++) {
      vec3 n = dNoiseD(p * frequency + float(i) * 7.31);
      sum.x += amplitude * n.x;
      sum.yz += amplitude * frequency * n.yz;
      amplitude *= 0.5;
      frequency *= 2.07;
    }
    return sum;
  }

  
  uniform sampler2D sceneDepth;
  uniform float softParticles;
  uniform vec2 depthResolution;
  uniform vec2 cameraRange;
  float dSoft(float viewDepth, float reach) {
    if (softParticles < 0.5 || reach <= 0.0) return 1.0;
    float depth = texture2D(sceneDepth, gl_FragCoord.xy / depthResolution).x;
    float surface = cameraRange.x * cameraRange.y / (cameraRange.y - depth * (cameraRange.y - cameraRange.x));
    return smoothstep(0.0, reach, surface - viewDepth);
  }

  void main() {
    float age = vState.x, seed = vState.y, opacity = vState.z, style = vState.w;
    vec2 q = vUv * 2.0 - 1.0;
    float r2 = dot(q, q);
    vec3 color;
    float alpha;
    if (style < 0.5) {
      // A ball of smoke eaten into by noise; the noise's slope bends its
      // normal, so the sun picks out billows and the far side falls into
      // the ambient. It thins and pales as it spreads.
      vec2 p = q * (1.2 + 0.3 * age) + vec2(seed * 37.0, seed * 11.0 - vLook.z * 0.1);
      vec3 n = dFbmD(p * 1.55);
      float density = 1.0 - r2 + (n.x - 0.47) * 1.3 - age * 0.25;
      alpha = smoothstep(0.0, 0.45, density) * smoothstep(0.0, 0.07, age) * pow(1.0 - age, 1.2);
      vec3 normal = normalize(vec3(q * 1.05 - n.yz * 0.28, sqrt(max(1.0 - r2, 0.0)) + 0.3));
      float lit = dot(normal, vSun) * 0.5 + 0.5;
      vec3 base = mix(vec3(0.06, 0.058, 0.056), vec3(0.54, 0.535, 0.525), clamp(vLook.x + age * 0.35, 0.0, 1.0));
      color = base * (ambient * (0.8 + 0.35 * n.x) + sunColor * (0.1 + 1.05 * lit * lit));
      color *= mix(0.72, 1.12, smoothstep(0.15, 0.85, n.x));
      // Fire beneath lights young smoke from below, for its first moments.
      color += vec3(1.0, 0.36, 0.07) * vLook.y * exp(-age * 10.0) * smoothstep(0.35, -0.9, q.y) * smoothstep(0.0, 0.5, density);
      alpha *= opacity * dSoft(vDepth, 0.45);
    } else if (style < 1.5) {
      // A flake of plating: ragged and dark, glinting as it turns.
      float n = dNoise(q * 2.4 + seed * 50.0);
      alpha = step(max(abs(q.x), abs(q.y)) + (n - 0.5) * 0.7, 0.72) * (1.0 - smoothstep(0.8, 1.0, age));
      float glint = pow(max(sin(vLook.w * 1.7 + seed * 30.0), 0.0), 12.0);
      color = vec3(0.1, 0.1, 0.11) * (ambient + sunColor) + sunColor * glint * 0.8;
      alpha *= opacity;
    } else {
      // A scorch mark: soot streaked out from the middle, fading slowly.
      float r = sqrt(r2);
      float streaks = dNoise(vec2(atan(q.y, q.x) * 3.2 + seed * 40.0, r * 2.0));
      float n = dNoise(q * 3.5 + seed * 19.0);
      alpha = smoothstep(1.0, 0.25, r + (streaks - 0.5) * 0.5 + (n - 0.5) * 0.3);
      alpha *= opacity * (1.0 - smoothstep(0.6, 1.0, age)) * smoothstep(0.0, 0.02, age);
      // Black soot, browner where it is streaked thin.
      color = mix(vec3(0.03, 0.028, 0.026), vec3(0.11, 0.075, 0.05), smoothstep(0.35, 0.9, streaks) * smoothstep(0.2, 0.8, r));
    }
    alpha *= smoothstep(0.25, 0.9, vDepth);
    float fog = clamp((vDepth - fogRange.x) / max(fogRange.y - fogRange.x, 1.0), 0.0, 1.0);
    color = mix(color, fogColor, fog);
    if (alpha < 0.003) discard;
    gl_FragColor = vec4(color, alpha);
  }
