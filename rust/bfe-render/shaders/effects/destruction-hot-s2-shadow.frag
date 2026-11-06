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
#define SHADER_NAME destruction-hot
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


  uniform vec2 fogRange;
  varying vec2 vUv;
  varying vec4 vState;
  varying float vTint;
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

  // Metal and gas as they cool: white-hot, yellow, orange, deep red.
  vec3 dHeat(float h) {
    vec3 c = mix(vec3(0.16, 0.012, 0.0), vec3(1.0, 0.2, 0.02), smoothstep(0.0, 0.3, h));
    c = mix(c, vec3(1.75, 0.68, 0.12), smoothstep(0.28, 0.62, h));
    return mix(c, vec3(2.5, 2.1, 1.45), smoothstep(0.64, 1.0, h));
  }
  void main() {
    float age = vState.x, seed = vState.y, intensity = vState.z, style = vState.w;
    vec2 q = vUv * 2.0 - 1.0;
    float r2 = dot(q, q);
    vec3 color;
    float alpha;
    float soft = 0.0;
    if (style < 0.5) {
      // Spark: a bright head fading down its tail, cooling as it flies.
      float head = vUv.y;
      alpha = pow(max(1.0 - abs(q.x), 0.0), 1.6) * smoothstep(0.0, 0.6, head) * (1.0 - smoothstep(0.7, 1.0, age));
      // Near the eye a spark's streak would cover the screen: fade it.
      alpha *= smoothstep(0.8, 2.5, vDepth);
      color = dHeat(0.88 - age * 0.75) * (0.7 + 0.5 * head);
    } else if (style < 1.5) {
      // Fire: dense billows burn white-yellow, the thin edges orange to red.
      vec3 n = dFbmD(q * 1.45 + vec2(seed * 23.0, seed * 7.0 - age * 2.4));
      float density = 1.0 - r2 * 1.1 + (n.x - 0.5) * 1.5 - age * 0.5;
      alpha = smoothstep(0.0, 0.32, density) * smoothstep(0.0, 0.05, age) * (1.0 - smoothstep(0.5, 1.0, age));
      float heat = clamp(density * 1.25 - age * 0.75 + 0.2, 0.0, 1.0);
      color = dHeat(heat) * (0.75 + 0.5 * n.x);
      soft = 0.3;
    } else if (style < 2.5) {
      // Flash: a blinding core in a soft halo, gone within a few frames.
      float core = exp(-r2 * 12.0), halo = exp(-r2 * 3.5);
      alpha = (core + halo * 0.3) * pow(1.0 - age, 2.0);
      vec3 warm = mix(vec3(1.7, 0.8, 0.3), vec3(2.6, 2.3, 1.9), core);
      vec3 cold = mix(vec3(0.45, 0.75, 2.2), vec3(2.3, 2.6, 3.0), core);
      color = mix(warm, cold, clamp(vTint, 0.0, 1.0));
      soft = 0.2;
    } else if (style < 3.5) {
      // The blast's light on the floor.
      alpha = exp(-r2 * 3.0) * pow(1.0 - age, 1.6) * smoothstep(1.0, 0.8, sqrt(r2));
      color = vec3(1.5, 0.75, 0.3);
    } else {
      // Ember: a glowing point that flickers and dims.
      float flicker = 0.55 + 0.45 * sin(seed * 40.0 + age * 31.0) * sin(seed * 13.0 + age * 17.0);
      alpha = exp(-r2 * 3.2) * flicker * smoothstep(0.0, 0.1, age) * (1.0 - smoothstep(0.55, 1.0, age));
      color = dHeat(0.78 - age * 0.45);
    }
    alpha *= intensity * dSoft(vDepth, soft) * smoothstep(0.25, 0.9, vDepth);
    alpha *= 1.0 - clamp((vDepth - fogRange.x) / max(fogRange.y - fogRange.x, 1.0), 0.0, 1.0);
    if (alpha < 0.002) discard;
    gl_FragColor = vec4(color * alpha, alpha);
  }
