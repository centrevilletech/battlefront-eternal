import * as THREE from 'three';
import { StormSky } from './storm-sky.js';
import { SkyProbe } from './sky-probe.js';
import { lighting } from './materials.js';
import { particleDepth } from './particle-depth.js';

// Concentric square rings share every edge. Detail follows the viewer, while
// waves and foam stay in world space. Cost does not depend on the map's size.
export function stormGeometry() {
  const sides = 64, rings = 128, stride = sides * 4;
  const positions = [0, 0, 0], indices = [];
  for (let ring = 1; ring <= rings; ring++) {
    const radius = ring <= 64 ? ring * 3 : 192 * (4096 / 192) ** ((ring - 64) / 64);
    for (let side = 0; side < 4; side++) for (let i = 0; i < sides; i++) {
      const t = -1 + 2 * i / sides;
      const x = [t, 1, -t, -1][side], z = [-1, t, 1, -t][side];
      positions.push(x * radius, 0, z * radius);
      const next = (side * sides + i + 1) % stride;
      const a = 1 + (ring - 1) * stride + side * sides + i, b = 1 + (ring - 1) * stride + next;
      if (ring === 1) indices.push(0, b, a);
      else { const c = a - stride, d = b - stride; indices.push(c, b, a, c, d, b); }
    }
  }
  const g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  g.setIndex(indices);
  return g;
}

// Deep-water dispersion. Horizontal steepness sums to < 1, so the surface
// cannot fold over itself. Derivatives supply smooth geometric normals and
// compression, which is also used by spray at its emission time.
const WAVES = /* glsl */`
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
`;
const NOISE = /* glsl */`
 float hash(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}
 float noise(vec2 p){vec2 i=floor(p),f=fract(p);f=f*f*(3.0-2.0*f);
   return mix(mix(hash(i),hash(i+vec2(1,0)),f.x),mix(hash(i+vec2(0,1)),hash(i+1.0),f.x),f.y);}
`;
const MASK = /* glsl */`
 uniform sampler2D mask;
 uniform vec2 gridOffset;
 uniform float gridWidth;
 float wet(vec2 p){vec2 uv=(p+gridOffset)/gridWidth;
   if(any(lessThan(uv,vec2(0)))||any(greaterThanEqual(uv,vec2(1))))return 0.0;
   return step(.5,texture2D(mask,uv).r);}
`;
const VERTEX = /* glsl */`
 ${WAVES}
 varying vec3 vWorld,vNormal;
 varying vec2 vRest;
 varying float vCrest;
 void main(){
   vec3 world=(modelMatrix*vec4(position,1)).xyz;
   vRest=world.xz;vec3 offset;
   sea(world.xz,time,offset,vNormal,vCrest);
   // The unresolved outermost rings disappear in sea haze, without a hard seam.
   offset*=1.0-smoothstep(1400.0,3500.0,length(position.xz));
   world+=offset;vWorld=world;
   gl_Position=projectionMatrix*viewMatrix*vec4(world,1);
 }
`;
const FRAGMENT = /* glsl */`
 ${WAVES}
 ${NOISE}
 ${MASK}
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
`;

const SPRAY_VERTEX = /* glsl */`
 ${WAVES}
 ${NOISE}
 ${MASK}
 attribute vec4 seed;
 uniform vec2 eye;
 uniform float seaLevel;
 varying vec2 vUv;
 varying float vAlpha,vDepth,vMist;
 void main(){
   vUv=uv;vMist=step(.78,seed.w);
   vec2 anchor=floor(eye/24.0)*24.0+(seed.xy-.5)*720.0;
   // Emission stays attached to a world cell through camera movement.
   anchor=floor(anchor/24.0)*24.0;
   float random=hash(anchor),life=mix(2.4,9.0,vMist);
   float cycle=time/life+random*19.0,age=fract(cycle),birth=time-age*life;
   vec3 offset,normal;float crest;
   sea(anchor,birth,offset,normal,crest);
   vec3 world=vec3(anchor.x,seaLevel,anchor.y)+offset;
   world.xz+=wind*age*mix(14.0,32.0,vMist);
   world.y+=mix(1.0+age*9.0-age*age*5.0,3.0+age*2.0,vMist);
   float fade=smoothstep(0.0,.13,age)*(1.0-smoothstep(.35,1.0,age));
   float edge=1.0-smoothstep(260.0,345.0,length(anchor-eye));
   vAlpha=fade*edge*wet(world.xz)*mix(smoothstep(.27,.46,crest)*.46,.09,vMist)*stormStrength;
   #ifndef STORM
     vAlpha*=1.0-vMist; // wind-blown mist belongs to the storm
   #endif
   vec4 view=viewMatrix*vec4(world,1);
   vec2 size=mix(vec2(3.0+age*10.0,2.0+age*7.0),vec2(35.0,7.0),vMist);
   view.xy+=position.xy*size;
   vDepth=-view.z;gl_Position=projectionMatrix*view;
 }
`;
const SPRAY_FRAGMENT = /* glsl */`
 ${NOISE}
 uniform float time,softParticles;
 uniform sampler2D sceneDepth;
 uniform vec2 depthResolution,cameraRange;
 uniform vec3 hazeColor;
 varying vec2 vUv;
 varying float vAlpha,vDepth,vMist;
 void main(){
   vec2 q=vUv*2.0-1.0;
   // Wind-torn spray: fine streaks across the card, not a soft white blob.
   float cloud=noise(vec2(vUv.x*3.0-time*.6,vUv.y*11.0))*.55+noise(vec2(vUv.x*7.0-time*.9,vUv.y*23.0)+3.7)*.45;
   float a=(1.0-smoothstep(.1,1.0,dot(q,q)))*smoothstep(.42,.8,cloud)*vAlpha*.75;
   if(softParticles>.5){float d=texture2D(sceneDepth,gl_FragCoord.xy/depthResolution).r;
     float z=cameraRange.x*cameraRange.y/(cameraRange.y-d*(cameraRange.y-cameraRange.x));
     a*=clamp((z-vDepth)/1.5,0.0,1.0);}
   a*=exp(-vDepth*.0015);
   if(a<.002)discard;
   gl_FragColor=vec4(mix(vec3(.6,.68,.72),hazeColor,vMist*.55),a);
 }
`;
// A horizon veil only in the far scene. Depth-tested behind scenery; the ocean
// uses the same extinction colour so water and sky meet without a black seam.
const HAZE_VERTEX = /* glsl */`
 varying vec3 vDirection;
 void main(){vDirection=position;vec3 world=position+cameraPosition;
   gl_Position=projectionMatrix*viewMatrix*vec4(world,1);}
`;
const HAZE_FRAGMENT = /* glsl */`
 uniform vec3 hazeColor;
 varying vec3 vDirection;
 void main(){float elevation=normalize(vDirection).y;
   float a=exp(-max(0.0,elevation)*12.0)*.9;
   gl_FragColor=vec4(hazeColor,a);}
`;

// The Enhanced ocean: waves for any sea, as big as its look's wind makes
// them, with crest spray. On a sea the map's weather rains on (Kamino) the
// storm comes with them: the storm sky's panorama and lightning
// (storm-sky.js), the horizon veil, rain rings and the storm's dark palette.
// A sea under a dry sky (Kashyyyk: Islands) keeps the map's own sky, which it
// reflects and fades into (sky-probe.js), in its look's colours and its sun.
export class EnhancedOcean {
 constructor(shared,graphics,{storm=false,look={}}={}) {
   this.uniforms={...shared,stormStrength:{value:Math.min(1,Math.max(.15,shared.amplitude.value/1.25))},rainRipples:{value:storm?1:0},
     hazeColor:{value:new THREE.Color(.065,.085,.11)},eye:{value:new THREE.Vector2()},seaLevel:{value:0},
     sceneDepth:particleDepth.map,softParticles:particleDepth.enabled,depthResolution:particleDepth.resolution,cameraRange:particleDepth.cameraRange};
   const defines=storm?{STORM:''}:{};
   this.storm=storm?new StormSky({graphics,uniforms:this.uniforms}):null;
   this.probe=storm?null:new SkyProbe(graphics);
   if(this.probe){
     const [r,g,b]=look.RefractionColor||[45,53,74],reflect=look.ReflectionColor?.[3]??255;
     Object.assign(this.uniforms,{skyProbe:{value:this.probe.texture},waterColor:{value:new THREE.Color(r/255,g/255,b/255)},
       reflectAmount:{value:reflect/255},ambient:lighting.ambient});
   }
   this.geometry=stormGeometry();
   this.material=new THREE.ShaderMaterial({defines,vertexShader:VERTEX,fragmentShader:FRAGMENT,uniforms:this.uniforms,side:THREE.DoubleSide});
   const quad=new THREE.PlaneGeometry(2,2),g=new THREE.InstancedBufferGeometry();
   g.setIndex(quad.index.clone());g.setAttribute('position',quad.attributes.position.clone());g.setAttribute('uv',quad.attributes.uv.clone());quad.dispose();
   const seeds=new Float32Array(384*4);
   for(let i=0;i<384;i++)seeds.set([(i%24+.5)/24,(Math.floor(i/24)+.5)/16,(i*.6180339)%1,(i*.7548776)%1],i*4);
   g.setAttribute('seed',new THREE.InstancedBufferAttribute(seeds,4));g.instanceCount=384;
   this.spray=new THREE.Mesh(g,new THREE.ShaderMaterial({defines,vertexShader:SPRAY_VERTEX,fragmentShader:SPRAY_FRAGMENT,uniforms:this.uniforms,transparent:true,depthWrite:false,side:THREE.DoubleSide,forceSinglePass:true}));
   this.spray.name='ocean-crest-spray';this.spray.frustumCulled=false;this.spray.renderOrder=19;
   this.spray.onBeforeRender=(...args)=>particleDepth.capture(...args);
   this.haze=null;
   if(storm){
     this.haze=new THREE.Mesh(new THREE.SphereGeometry(850,32,12),new THREE.ShaderMaterial({vertexShader:HAZE_VERTEX,fragmentShader:HAZE_FRAGMENT,uniforms:this.uniforms,transparent:true,depthWrite:false,side:THREE.BackSide}));
     this.haze.name='ocean-horizon-haze';this.haze.frustumCulled=false;this.haze.renderOrder=10;
   }
 }
 setEnabled(enabled){
   this.spray.visible=enabled;if(this.haze)this.haze.visible=enabled;
   this.storm?.setEnabled(enabled);if(this.probe)this.probe.enabled=enabled;
 }
 update(camera,height){
   this.uniforms.eye.value.set(camera.position.x,camera.position.z);this.uniforms.seaLevel.value=height;
   this.storm?.update(this.uniforms.time.value,camera);this.probe?.update(this.uniforms.time.value,camera);
 }
 dispose(){
   this.storm?.dispose();this.probe?.dispose();
   for(const mesh of [this.spray,this.haze])if(mesh){mesh.removeFromParent();mesh.geometry.dispose();mesh.material.dispose();}
   this.geometry.dispose();this.material.dispose();
 }
}
