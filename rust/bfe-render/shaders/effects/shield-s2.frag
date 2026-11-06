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
#define SHADER_NAME droideka-energy-shield
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


  uniform sampler2D fieldMap;
  uniform float hasMap, shieldTime, shieldFade, charge, enhanced, birth, inside, flash;
  uniform vec3 fieldColor, specularColor;
  // The map's sun (materials.js lighting), each on a line of its own.
  uniform vec3 sunDirection;
  uniform vec3 sunColor;
  uniform vec4 impacts[6];
  uniform float powers[6], blasts[6];
  varying vec2 vUv;
  varying vec3 vLocal, vNormal, vView;

  float hash(vec3 p) { p = fract(p * 0.3183099 + vec3(0.1, 0.2, 0.3)); p *= 17.0; return fract(p.x * p.y * p.z * (p.x + p.y + p.z)); }
  float noise(vec3 p) {
    vec3 i = floor(p), f = fract(p); f = f * f * (3.0 - 2.0 * f);
    return mix(mix(mix(hash(i), hash(i+vec3(1,0,0)), f.x), mix(hash(i+vec3(0,1,0)), hash(i+vec3(1,1,0)), f.x), f.y),
      mix(mix(hash(i+vec3(0,0,1)), hash(i+vec3(1,0,1)), f.x), mix(hash(i+vec3(0,1,1)), hash(i+vec3(1,1,1)), f.x), f.y), f.z);
  }

  float cells(vec2 p) {
    vec2 period = vec2(1.7320508, 1.0);
    vec2 a = mod(p, period) - period * 0.5;
    vec2 b = mod(p - period * 0.5, period) - period * 0.5;
    vec2 h = abs(dot(a,a) < dot(b,b) ? a : b);
    float d = max(dot(h, vec2(0.8660254, 0.5)), h.y);
    float aa = max(fwidth(d), 0.012);
    return smoothstep(0.5 - aa * 1.4, 0.5, d);
  }
  void main() {
    // One shell face, including when the first-person camera is inside.
    if ((inside < 0.5 && !gl_FrontFacing) || (inside > 0.5 && gl_FrontFacing)) discard;
    // The interior covers the viewport. Keep a faint field and localized
    // damage feedback without running exterior plasma, lattice, specular and
    // expanding-ring work for every pixel the player aims through. Seen from
    // inside the shell is nearly face-on everywhere, so its rim term is no
    // cue there: a light veil, clear in the middle of the view and thicker
    // toward its edges, with the field's drifting cloud, shows it is up.
    if (inside > 0.5 && enhanced > 0.5) {
      vec3 p = normalize(vLocal);
      float hit = flash * 0.2;
      for (int i = 0; i < 6; i++) {
        float age = shieldTime - impacts[i].w;
        if (age >= 0.0 && age < 1.15) {
          float proximity = max(dot(p, impacts[i].xyz), 0.0);
          float spot = pow(proximity, blasts[i] > 0.5 ? 8.0 : 64.0);
          hit += spot * exp(-age * 9.0) * powers[i];
        }
      }
      float edge = smoothstep(0.2, 0.8, length(normalize(vView).xy));
      float cloud = mix(0.48, texture2D(fieldMap, vUv + vec2(shieldTime, 0.0)).r, hasMap);
      float veil = (0.04 + edge * 0.24) * (0.45 + cloud * 1.15);
      float alpha = (veil + exp(-birth * 12.0) * 0.1 + hit * 0.16) * shieldFade;
      if (alpha < 0.002) discard;
      gl_FragColor = vec4(mix(vec3(0.12, 0.52, 0.9), vec3(0.38, 0.83, 1.0), edge) + hit * vec3(0.3, 0.5, 0.6), min(alpha, 0.34));
      return;
    }
    vec3 n = normalize(vNormal), eye = normalize(-vView), p = normalize(vLocal);
    float facing = abs(dot(n, eye));
    vec4 cloud = mix(vec4(0.48, 0.48, 0.48, 0.74), texture2D(fieldMap, vUv + vec2(shieldTime, 0.0)), hasMap);
    float strength = 1.0 - (1.0 - charge) * (1.0 - charge);
    vec3 light = normalize(mat3(viewMatrix) * -sunDirection);
    float highlight = pow(max(dot(reflect(-light, n), eye), 0.0), 72.0);
    // Native droideka shader uses 1 - 1.5*(N.V)^2 and scrolls one UV/s.
    float classicRim = max(1.0 - 1.5 * facing * facing, 0.0);
    float alpha = classicRim * cloud.a * shieldFade * strength * 0.68;
    vec3 color = fieldColor * (0.65 + cloud.rgb * 0.7) + specularColor * highlight * sunColor * 1.7;
    float impactLight = 0.0, impactCore = 0.0, blastLight = 0.0;
    for (int i = 0; i < 6; i++) {
      float age = shieldTime - impacts[i].w;
      if (age >= 0.0 && age < 1.15) {
        float arc = acos(clamp(dot(p, impacts[i].xyz), -1.0, 1.0));
        if (enhanced > 0.5 && blasts[i] > 0.5) {
          // A broad pressure front washes around the shell, with a trailing
          // ring and a brief flare across the hemisphere facing the blast.
          float front = exp(-pow((arc - age * 5.0) / (0.12 + age * 0.14), 2.0));
          float wake = exp(-pow((arc - age * 3.4) / 0.09, 2.0));
          float wash = exp(-arc * arc / 1.5) * exp(-age * 8.0);
          blastLight += ((front + wake * 0.45) * exp(-age * 3.5) + wash * 0.8) * powers[i];
        }
        float ring = exp(-pow((arc - age * 2.9) / (0.028 + age * 0.065), 2.0));
        float echo = exp(-pow((arc - age * 1.8) / (0.022 + age * 0.04), 2.0));
        impactLight += (ring + echo * 0.35) * exp(-age * 3.0) * powers[i];
        impactCore += exp(-arc * arc / 0.025) * exp(-age * 9.0) * powers[i];
      }
    }
    if (enhanced > 0.5) {
      float rim = pow(1.0 - facing, 2.2);
      float edge = pow(1.0 - facing, 9.0);
      // Triplanar cell detail avoids a pinched grid at the sphere poles.
      vec3 weights = pow(abs(p), vec3(5.0));
      weights /= max(weights.x + weights.y + weights.z, 0.001);
      float lattice = dot(vec3(cells(p.yz * 19.0), cells(p.xz * 19.0), cells(p.xy * 19.0)), weights);
      float flow = 0.5 + 0.5 * sin(p.y * 36.0 - shieldTime * 3.0 + sin(p.x * 8.0 + shieldTime) * 0.6);
      float sweep = exp(-pow((p.y - (birth * 6.0 - 1.3)) / 0.10, 2.0)) * exp(-birth * 2.0);
      float pop = exp(-birth * 12.0);
      float stress = (1.0 - charge) * (0.5 + 0.5 * sin(shieldTime * 19.0 + p.y * 31.0));
      // Wandering plasma threads energize patches of the field. Using the
      // sphere direction avoids UV seams and keeps the clear center readable.
      float turbulence = noise(p * 9.0 + vec3(shieldTime * 0.37, -shieldTime * 0.23, shieldTime * 0.14));
      float current = p.y * 4.0 + p.x * 1.8 + turbulence * 1.5 - shieldTime * 0.42;
      float thread = 1.0 - smoothstep(0.012, 0.055 + fwidth(current), abs(fract(current) - 0.5));
      float energized = smoothstep(0.54, 0.79, noise(p * 3.0 + shieldTime * 0.24));
      float plasma = thread * energized * (0.18 + rim * 0.82);
      float detail = lattice * (rim * 0.18 + impactLight * 0.34 + blastLight * 0.75 + pop * 0.28 + sweep * 0.3);
      alpha = (0.010 + rim * 0.31 + edge * 0.25 + detail + plasma * 0.42 + flow * rim * 0.04 + sweep * 0.85 + pop * 0.30) * shieldFade * (0.7 + strength * 0.3);
      color = mix(vec3(0.08, 0.40, 0.78), vec3(0.38, 0.83, 1.0), rim);
      color += vec3(0.32, 0.67, 0.93) * (detail * 2.0 + sweep * 1.8 + pop);
      color += vec3(0.28, 0.72, 1.0) * edge * 1.6;
      color += vec3(0.35, 0.85, 1.4) * plasma * 2.8;
      color += specularColor * highlight * 0.7;
      color = mix(color, vec3(0.45, 0.60, 1.0), stress * 0.16);
      alpha += blastLight * shieldFade * 0.65;
      color += vec3(0.55, 0.85, 1.0) * blastLight * 1.8;
      // A faint moving interference field retains a clear center.
      alpha += cloud.r * 0.012 * shieldFade;
    }
    float hit = (impactLight * 0.75 + impactCore + flash * (0.2 + classicRim * 0.8)) * shieldFade;
    alpha += hit * mix(0.24, 0.8, enhanced);
    color += vec3(0.6, 0.9, 1.0) * hit * mix(0.6, 1.5, enhanced);
    // Inside the field, keep the player's view readable.
    alpha *= mix(1.0, 0.22, inside);
    if (alpha < 0.002) discard;
    gl_FragColor = vec4(color, clamp(alpha, 0.0, 0.88));
  }
