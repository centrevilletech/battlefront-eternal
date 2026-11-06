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
#define FLIP_SIDED
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
uniform sampler2D sceneDepth, reflection;
uniform mat4 reflectionMatrix;
uniform vec2 volumeResolution,cameraRange;
uniform float volumeTime,hasDepth,hasReflection;
uniform vec4 wakes[8], motion[8];
varying vec3 point,eye,base,rightAxis,upAxis,forwardAxis,size;
varying float type;
vec3 worldAt(vec3 p){return base+rightAxis*p.x*size.x+upAxis*p.y*size.y+forwardAxis*p.z*size.z;}
float noise3(vec3 p){return texture(noiseField,p/32.).r;}
void main(){
  vec3 ray=normalize(point-eye),inv=1./(ray+vec3(1e-7));
  vec3 a=(vec3(-.5,0.,-.5)-eye)*inv,b=(vec3(.5,1.,.5)-eye)*inv;
  vec3 lo=min(a,b),hi=max(a,b);
  float start=max(0.,max(lo.x,max(lo.y,lo.z))),finish=min(hi.x,min(hi.y,hi.z));
  float depthRate=-(viewMatrix*vec4(worldAt(eye+ray)-cameraPosition,0.)).z;
  start=max(start,cameraRange.x/max(depthRate,1e-5));
  if(finish<=start)discard;
  float surface=1e6;
  if(hasDepth>.5){float z=texture2D(sceneDepth,gl_FragCoord.xy/volumeResolution).r;
    surface=cameraRange.x*cameraRange.y/(cameraRange.y-z*(cameraRange.y-cameraRange.x));}
  float samples=type>2.5?24.:(distance(cameraPosition,base)>35.?12.:18.);
  float stepSize=(finish-start)/samples,meters=length(ray*size)*stepSize,trans=1.;vec3 color=vec3(0.);
  for(int i=0;i<24;i++){
    if(float(i)>=samples)break;
    vec3 p=eye+ray*(start+(float(i)+.5)*stepSize),world=worldAt(p);
    float depth=-(viewMatrix*vec4(world,1.)).z;if(depth>surface)break;
    vec3 local=p*size;
    float height=p.y;
    vec2 bend=vec2(sin(local.y*.9-volumeTime*.7+base.x),cos(local.y*.7-volumeTime*.6+base.z))*local.y*.13;
    float radius=type<.5?size.x*(.10+.25*height):mix(.09,.75,height);
    if(type>1.5)radius=size.x*.37;
    float radial=length(local.xz-bend)/max(radius,.05);
    // Noise is in [0,1], so even maximal erosion cannot put density here.
    // Skip texture fetches and character-wake work outside the actual plume.
    if(radial>=1.81)continue;
    vec3 flow=world*2.4-upAxis*volumeTime*(type<.5?1.15:3.6)+vec3(volumeTime*.16,0.,volumeTime*.11);
    float n=noise3(flow),fine=noise3(flow*2.07+vec3(11.,7.,3.));
    float edge=1.-smoothstep(.35,1.25,radial+(n-.5)*1.1);
    float breakup=smoothstep(.28,.66,n*.72+fine*.28);
    float density=edge*breakup*smoothstep(0.,.055,height)*(1.-smoothstep(.45,1.,height));
    density*=type<.5?.65:type<1.5?1.7:.16;
    if(density<=0.)continue;
    float light=clamp(.65+(noise3(flow+vec3(0.,2.,0.))-n)*1.2,.45,1.);
    // Cool, translucent steam. A restrained warm contribution stays close
    // to the grates instead of tinting the entire cloud tan.
    vec3 tint=vec3(.78,.84,.91)*light;
    if(type<.5)tint+=vec3(.14,.045,0.)*exp(-local.y*1.8);
    if(type<2.5){
      for(int j=0;j<8;j++){
        float age=volumeTime-wakes[j].w;
        if(age>=0.&&age<2.){
          vec3 center=wakes[j].xyz+motion[j].xyz*age*.18;
          vec3 delta=world-center;delta.y-=clamp(delta.y,.15,1.65);
          float r=length(delta),radius=.55+age*.42;
          float strength=motion[j].w*exp(-age*1.8);
          density*=1.-clamp(exp(-r*r/(radius*radius))*strength*.94,0.,.98);
          // Displaced steam curls around the wake instead of simply vanishing.
          density*=1.+.35*strength*exp(-pow((r-radius)*3.,2.))*n;
        }
      }
    }
    density*=smoothstep(0.,.12,surface-depth);
    float alpha=1.-exp(-density*meters);color+=trans*alpha*tint;trans*=1.-alpha;
    if(trans<.035)break;
  }
  if(1.-trans<.002)discard;gl_FragColor=vec4(color,1.-trans);
}