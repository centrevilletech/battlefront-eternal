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
#define STORM 
#define DOUBLE_SIDED
uniform mat4 viewMatrix;
uniform vec3 cameraPosition;
uniform bool isOrthographic;
#define OPAQUE
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


 
 uniform vec2 wind;
 uniform float time, stormStrength;
 void swell(vec2 p, float t, vec2 dir, float length, float amp, float steep,
            inout vec3 offset, inout vec3 dx, inout vec3 dz, inout float compression) {
   float k=6.2831853/length, phase=k*dot(dir,p)-sqrt(9.81*k)*t;
   float range=distance(p,cameraPosition.xz);
   float spacing=mix(3.0,range*.05,smoothstep(170.0,210.0,range));
   float resolved=1.0-smoothstep(length*.18,length*.5,spacing);
   float s=sin(phase), c=cos(phase), a=amp*stormStrength*resolved, q=steep*stormStrength*resolved;
   offset+=vec3(dir.x*q/k*c,a*s,dir.y*q/k*c);
   dx+=vec3(-dir.x*dir.x*q*s,dir.x*k*a*c,-dir.x*dir.y*q*s);
   dz+=vec3(-dir.x*dir.y*q*s,dir.y*k*a*c,-dir.y*dir.y*q*s);
   compression+=q*s;
 }
 void sea(vec2 p,float t,out vec3 offset,out vec3 normal,out float crest) {
   vec3 dx=vec3(1,0,0), dz=vec3(0,0,1);offset=vec3(0);crest=0.0;
   vec2 across=vec2(-wind.y,wind.x);
   swell(p,t,wind,78.0,3.2,.28,offset,dx,dz,crest);
   swell(p,t,normalize(wind+across*.52),43.0,1.9,.23,offset,dx,dz,crest);
   swell(p,t,normalize(wind-across*.65),27.0,1.05,.17,offset,dx,dz,crest);
   swell(p,t,normalize(wind+across*1.3),15.0,.52,.095,offset,dx,dz,crest);
   swell(p,t,normalize(wind-across*.3),8.1,.23,.045,offset,dx,dz,crest);
   normal=normalize(cross(dz,dx));
 }

 
 float hash(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}
 float noise(vec2 p){vec2 i=floor(p),f=fract(p);f=f*f*(3.0-2.0*f);
   return mix(mix(hash(i),hash(i+vec2(1,0)),f.x),mix(hash(i+vec2(0,1)),hash(i+1.0),f.x),f.y);}

 
 uniform sampler2D mask;
 uniform vec2 gridOffset;
 uniform float gridWidth;
 float wet(vec2 p){vec2 uv=(p+gridOffset)/gridWidth;
   if(any(lessThan(uv,vec2(0)))||any(greaterThanEqual(uv,vec2(1))))return 0.0;
   return step(.5,texture2D(mask,uv).r);}

 uniform vec3 sunDirection,sunColor;
 uniform float rainRipples;
 #ifdef STORM
   uniform vec3 hazeColor,lightningPosition;
   uniform sampler2D cloudMap;
   uniform float cloudReady,lightningFlash;
 #else
   uniform samplerCube skyProbe;
   uniform vec3 waterColor,ambient;
   uniform float reflectAmount;
 #endif
 varying vec3 vWorld,vNormal;
 varying vec2 vRest;
 varying float vCrest;
 // Raindrop rings on a jittered grid, each cell on its own clock: a slope
 // from the expanding crest of the newest ring (Lagarde's rain ripples).
 vec2 rain(vec2 p,float t){
   vec2 slope=vec2(0);
   for(int layer=0;layer<2;layer++){
     vec2 q=p*(layer==0?1.7:2.9)+float(layer)*17.3;
     vec2 cell=floor(q),f=fract(q)-.5;
     float h=hash(cell),age=fract(t*(1.1+h*.6)+h*7.3);
     vec2 center=vec2(hash(cell+3.1),hash(cell+7.7))-.5;
     vec2 d=f-center*.6;float r=length(d),front=age*.45;
     float ring=sin(clamp((r-front)*38.0,-3.14159,0.0))*(1.0-age)*smoothstep(.5,.3,r);
     slope+=d/max(r,.001)*ring*.35;
   }
   return slope;
 }
 void main(){
   if(wet(vWorld.xz)<.5)discard;
   vec3 V=normalize(cameraPosition-vWorld);
   float distanceToEye=length(cameraPosition-vWorld);
   vec2 across=vec2(-wind.y,wind.x);
   // Fine capillary waves fade before they can shimmer at a grazing angle.
   float detail=1.0-smoothstep(90.0,550.0,distanceToEye);
   vec2 ripple=(vec2(noise(vRest*.65-wind*time*.8),
                           noise(vRest*.83+across*7.4-wind*time*.9))-.5)*.22;
   #ifndef STORM
     // Under a dry sky the wind alone raises them: faint on a calm sea.
     ripple*=mix(.4,1.0,stormStrength);
   #endif
   ripple+=rain(vRest,time)*rainRipples*(1.0-smoothstep(12.0,45.0,distanceToEye));
   vec3 N=normalize(vNormal+vec3(ripple.x,0,ripple.y)*detail);
   float facing=max(dot(N,V),0.0);
   float fresnel=.02+.98*pow(1.0-facing,5.0);
   vec3 R=reflect(-V,N);
   #ifdef STORM
   float clouds=noise(R.xz/(abs(R.y)+.3)*2.0+wind*time*.018);
   vec3 reflection=mix(vec3(.025,.04,.065),vec3(.18,.24,.30),clouds);
   reflection=mix(hazeColor,reflection,smoothstep(0.0,.65,abs(R.y)));
   if(cloudReady>.5){
     vec2 skyUv=vec2(atan(R.x,R.z)/6.2831853+.5,asin(clamp(abs(R.y),0.0,1.0))/1.5707963);
     reflection=texture2D(cloudMap,skyUv).rgb;
   }
   // Light through the water: the storm's brightest sky low over the sea (or
   // a lightning strike) shines through thin crests as a green-teal glow, and
   // the sea body scatters a little of it back (the Atlas water model).
   vec3 strike=normalize(lightningPosition-vWorld);
   vec3 L=normalize(mix(normalize(vec3(-wind.x,.38,-wind.y)),strike,lightningFlash));
   float crest=smoothstep(.04,.55,vCrest);
   float through=pow(max(dot(L,-V)*.5+.5,0.0),3.0)*pow(.5-.5*dot(L,vNormal),2.0)*crest;
   float up=max(dot(N,normalize(vec3(-.3,1,.15))),0.0);
   vec3 scatter=vec3(.03,.058,.068)*(.55+.45*up)+vec3(.04,.19,.17)*(through*1.6+crest*.22)*(1.0+lightningFlash*3.0);
   vec3 color=(1.0-fresnel)*scatter+fresnel*reflection;
   color+=vec3(.36,.48,.61)*lightningFlash*(.05+pow(max(dot(R,strike),0.0),22.0)*.7);
   vec3 Ls=normalize(-sunDirection);
   color+=sunColor*pow(max(dot(N,normalize(V+Ls)),0.0),96.0)*.18;
   #else
   // A sea the map doesn't storm over, under its own sky (sky-probe.js): it
   // reflects that sky (softened, as rippled water does), as much as its
   // look's ReflectionColor lets it, and the sun lights its body (the look's
   // RefractionColor) and shines through thin crests toward the eye.
   vec3 reflection=textureCube(skyProbe,vec3(R.x,abs(R.y),R.z),2.0).rgb*reflectAmount;
   vec3 Ls=normalize(-sunDirection);
   float crest=smoothstep(.04,.55,vCrest);
   float through=pow(max(dot(Ls,-V)*.5+.5,0.0),3.0)*pow(.5-.5*dot(Ls,vNormal),2.0)*crest;
   float up=max(dot(N,Ls),0.0);
   vec3 scatter=waterColor*(ambient+sunColor*up)+waterColor*sunColor*(through*1.6+crest*.22)*2.0;
   vec3 color=(1.0-fresnel)*scatter+fresnel*reflection;
   color+=sunColor*pow(max(dot(N,normalize(V+Ls)),0.0),96.0)*.45;
   #endif
   // Whitecaps break on compressed crests and erode as connected patches of
   // turbulent foam (coverage eats into a foam pattern, as in Crest); behind
   // them the foam is drawn out into long streaks along the wind.
   vec2 drift=vRest-wind*time*.65;
   float breakup=noise(drift*.11)*.65+noise(drift*.39)*.35;
   vec2 capUv=vec2(dot(drift,wind)*.55,dot(drift,across)*1.1);
   float turbulence=noise(capUv)*.5+noise(capUv*2.7+3.1)*.3+noise(capUv*6.1-1.7)*.2;
   float coverage=smoothstep(.2,.6,vCrest+(breakup-.5)*.3);
   float aa=max(fwidth(turbulence),.03);
   float foam=smoothstep(1.0-coverage-aa,1.12-coverage+aa,turbulence);
   vec2 streakUv=vec2(dot(vRest,wind)/30.0,dot(vRest,across)/3.2)+vec2(-time*.22,0.0);
   float streak=noise(streakUv)*.6+noise(streakUv*vec2(2.3,3.1)+5.2)*.4;
   float trail=smoothstep(.06,.3,vCrest)*smoothstep(.56,.8,streak)*smoothstep(.3,.65,breakup);
   foam=max(foam,trail*.5);
   #ifdef STORM
   vec3 foamColor=vec3(.78,.85,.88)*(.65+.35*up)+vec3(.3,.38,.45)*lightningFlash;
   color=mix(color,foamColor,clamp(foam,0.0,1.0)*.95);
   float fog=1.0-exp(-distanceToEye*.0016);
   gl_FragColor=vec4(mix(color,hazeColor,fog),1);
   #else
   vec3 foamColor=(ambient+sunColor*up)*1.1;
   color=mix(color,foamColor,clamp(foam,0.0,1.0)*.95);
   // Far off it takes the colour of its sky at the horizon behind it.
   vec3 horizon=textureCube(skyProbe,normalize(vec3(-V.x,.03,-V.z))).rgb;
   float fog=1.0-exp(-distanceToEye*.0005);
   gl_FragColor=vec4(mix(color,horizon,fog),1);
   #endif
 }
