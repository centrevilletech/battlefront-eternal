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


  uniform highp sampler3D noiseField;
  uniform sampler2D sceneDepth;
  uniform vec2 depthResolution, cameraRange;
  uniform float flameTime, hasDepth;
  varying vec3 boxPoint, rayOrigin, axisRight, axisUp, axisForward, origin;
  varying vec2 extent, curvature;
  varying float radius, lengthJet, seed;
  vec3 worldAt(vec3 p) { return origin+axisRight*p.x*extent.x+axisUp*p.y*extent.y+axisForward*p.z*lengthJet; }
  float noise3(vec3 p) { return texture(noiseField,p/32.0).r; }
  void main() {
    bool within = all(greaterThan(rayOrigin,vec3(-1.0,-1.0,0.0))) && all(lessThan(rayOrigin,vec3(1.0)));
    if ((!within && !gl_FrontFacing) || (within && gl_FrontFacing)) discard;
    vec3 ray = normalize(boxPoint-rayOrigin);
    vec3 inv = 1.0 / (ray + vec3(1e-7));
    vec3 a = (vec3(-1.0,-1.0,0.0)-rayOrigin)*inv;
    vec3 b = (vec3(1.0)-rayOrigin)*inv;
    vec3 lo = min(a,b), hi=max(a,b);
    float start = max(0.0,max(lo.x,max(lo.y,lo.z)));
    float finish = min(hi.x,min(hi.y,hi.z));
    if (finish <= start) discard;
    float surface = 1e6;
    if (hasDepth > 0.5) {
      float z=texture2D(sceneDepth,gl_FragCoord.xy/depthResolution).x;
      surface=cameraRange.x*cameraRange.y/(cameraRange.y-z*(cameraRange.y-cameraRange.x));
    }
    // Fixed midpoint samples: no screen-space random jitter or temporal shimmer.
    float stepSize=(finish-start)/28.0;
    float meters=length(vec3(ray.xy*extent,ray.z*lengthJet))*stepSize;
    vec3 emission=vec3(0.0); float transmittance=1.0;
    for(int i=0;i<28;i++) {
      vec3 p=rayOrigin+ray*(start+(float(i)+0.5)*stepSize);
      float depth=-(viewMatrix*vec4(worldAt(p),1.0)).z;
      if (depth>surface) break;
      float z=clamp(p.z,0.0,1.0), spread=smoothstep(0.025,0.30,z);
      vec2 center=curvature*z*z;
      // Advected eddies become larger and less coherent downstream.
      vec3 field=vec3(p.xy*extent*42.0,z*9.0-flameTime*28.0+seed);
      float coarse=noise3(field);
      float fine=noise3(field*2.31+vec3(7.0,13.0,-flameTime*3.0));
      center+=vec2(sin(z*13.0-flameTime*5.0+seed),cos(z*17.0-flameTime*4.0+seed))*radius*0.20*spread*z;
      float r=radius*(0.90+spread*1.1)*(1.0-0.66*smoothstep(0.48,1.0,z));
      float radial=length(p.xy*extent-center)/max(r,0.001);
      float breakup=(coarse-0.46)*1.55+(fine-0.5)*0.50;
      float body=1.0-smoothstep(0.45,1.12,radial+breakup*spread);
      float tail=1.0-smoothstep(0.53+coarse*0.18,0.89+fine*0.10,z);
      float lip=smoothstep(0.0,0.012,z);
      float core=exp(-radial*radial*3.2)*(1.0-smoothstep(0.26,0.76,z));
      core*=0.84+0.16*cos(z*51.0-flameTime*12.0+seed);
      // Dense hot root; separate, cooler billowing combustion envelope.
      float density=(body*(4.0+coarse*8.0)*tail+core*42.0)*lip;
      density*=smoothstep(0.0,0.014,surface-depth);
      float alpha=1.0-exp(-density*meters);
      vec3 color=mix(vec3(1.5,0.32+fine*0.24,0.018),vec3(2.5,2.15,1.6),core);
      color+=vec3(0.12,0.08,0.02)*body*coarse;
      // Radiance from the dense nozzle core shines through the translucent
      // combustion layer instead of being tinted into an opaque orange torch.
      emission+=transmittance*(alpha*color+vec3(1.0,0.88,0.63)*core*lip*meters*14.0);
      transmittance*=1.0-alpha;
      if(transmittance<0.02)break;
    }
    float opacity=1.0-transmittance;
    if(opacity<0.003)discard;
    gl_FragColor=vec4(emission,opacity);
  }
