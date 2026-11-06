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


  uniform float kind;
  uniform vec3 fogColor;
  uniform vec2 fogRange;
  uniform sampler2D sceneDepth;
  uniform float softParticles;
  uniform vec2 depthResolution;
  uniform vec2 cameraRange;
  varying vec2 wakeUv;
  varying vec4 wakeState;
  varying float viewDepth;
  float hash(vec2 p) { return fract(sin(dot(p,vec2(127.1,311.7))) * 43758.5453); }
  float noise(vec2 p) {
    vec2 i=floor(p),f=fract(p);f=f*f*(3.0-2.0*f);
    return mix(mix(hash(i),hash(i+vec2(1,0)),f.x),mix(hash(i+vec2(0,1)),hash(i+vec2(1,1)),f.x),f.y);
  }
  void main() {
    float age = wakeState.x, seed = wakeState.y;
    vec2 q = wakeUv * 2.0 - 1.0;
    float radial = max(0.0, 1.0 - dot(q,q));
    float alpha = radial * radial * smoothstep(0.0, 0.08, age) * pow(1.0-age, 1.5);
    vec3 color;
    if (kind > 1.5) {
      color = mix(vec3(1.6,0.75,0.19), vec3(0.7,0.16,0.025), age);
    } else {
      vec2 advect = vec2(seed, -wakeState.w * 2.0);
      vec2 p = wakeUv * vec2(5.0,8.0) + advect;
      float bend = noise(p * 0.53 + seed) - 0.5;
      p.x += bend * 1.4;
      float detail = noise(p) * 0.6 + noise(p * 2.03 + 7.1) * 0.4;
      // Two wandering filaments break up the outline; empty regions remain
      // transparent rather than reading as uniformly filled smoke balloons.
      float filament = 1.0-smoothstep(0.08,0.48,abs(q.x + bend * 0.8 + sin(q.y*4.0+seed+age*2.0)*0.23));
      alpha *= kind < 0.5
        ? smoothstep(0.14,0.65,detail) * mix(0.45,1.0,filament)
        : smoothstep(0.24,0.69,detail) * mix(0.25,1.0,filament);
      if (kind < 0.5) {
        float heat = 1.0-smoothstep(0.20,0.85,age);
        color = mix(vec3(0.28,0.10,0.035),vec3(2.1,0.72,0.08),heat);
        color += vec3(1.2,0.82,0.3) * pow(detail,3.0) * heat;
      } else {
        color = mix(vec3(0.25,0.27,0.29),vec3(0.47,0.45,0.40),detail);
        // A little residual heat at the start, quickly cooling into vapor.
        color += vec3(0.09,0.035,0.006) * pow(1.0-age,5.0);
      }
    }
    alpha *= wakeState.z * smoothstep(0.2,0.8,viewDepth);
    if (softParticles > 0.5) {
      float z=texture2D(sceneDepth,gl_FragCoord.xy/depthResolution).x;
      float surface=cameraRange.x*cameraRange.y/(cameraRange.y-z*(cameraRange.y-cameraRange.x));
      alpha *= smoothstep(0.0,kind>1.5?0.035:0.14,surface-viewDepth);
    }
    if (alpha < 0.0015) discard;
    float fog=clamp((viewDepth-fogRange.x)/max(1.0,fogRange.y-fogRange.x),0.0,1.0);
    if(kind>0.5&&kind<1.5) color=mix(color,fogColor,fog);
    else alpha*=1.0-fog;
    // Premultiply emissive radiance before the normalized target clamps it.
    // Otherwise hot red (>1) clips first and the fire turns muddy yellow.
    gl_FragColor=vec4(kind>0.5&&kind<1.5 ? color : color*alpha,alpha);
  }
