import * as THREE from 'three';
import { lighting } from './materials.js';
import { drawFramebuffer } from './render-targets.js';
import { JET_APEX, TIER } from './fountain-jets.js';

const vertexShader=/* glsl */`
varying vec3 world;
void main(){world=(modelMatrix*vec4(position,1.)).xyz;gl_Position=projectionMatrix*viewMatrix*vec4(world,1.);}`;
const fragmentShader=/* glsl */`
uniform mat4 projectionMatrix;
uniform sampler2D reflection,sceneColor,sceneDepth,foamMap;
uniform mat4 reflectionMatrix;
uniform vec2 waterResolution,cameraRange;
uniform float waterTime,hasReflection,hasFoam,hasTier;
uniform vec3 sky,waterSunDirection,waterSunColor;
uniform vec4 wakes[8],motion[8],basins[5];
varying vec3 world;
const float APEX=${JET_APEX.toFixed(2)},LIP=${TIER.lip.toFixed(2)},RISE=${TIER.rise.toFixed(3)},FOOT=${TIER.foot.toFixed(2)},SIDES=${TIER.sides.toFixed(1)};
float linearDepth(float z){return cameraRange.x*cameraRange.y/(cameraRange.y-z*(cameraRange.y-cameraRange.x));}
float hash(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}
float noise(vec2 p){vec2 i=floor(p),f=fract(p);f=f*f*(3.-2.*f);
 return mix(mix(hash(i),hash(i+vec2(1.,0.)),f.x),mix(hash(i+vec2(0.,1.)),hash(i+1.),f.x),f.y);}
// Sunlight focused by the moving surface onto the floor beneath it.
float caustic(vec2 p,float t){
 vec2 i=p;float c=0.;
 for(int n=0;n<4;n++){float s=t*(1.-3.5/float(n+2));i=p+vec2(cos(s-i.x)+sin(s+i.y),sin(s-i.y)+cos(s+i.x));
  c+=1./length(vec2(p.x/(sin(i.x+s)*200.),p.y/(cos(i.y+s)*200.)));}
 return pow(clamp(1.17-pow(c*.25,1.4),0.,1.),7.);
}
// How far out from a basin's centre, measured as the radius of the tier's
// 12-sided outline through this point: LIP on its lip, FOOT at its foot.
float polygon(vec2 delta){float k=6.2831853/SIDES;return length(delta)*cos(mod(atan(delta.y,delta.x),k)-.5*k)/cos(.5*k);}
// Rings spreading from drops that land at random spots, one per cell.
vec2 rings(vec2 p,float rate){
 vec2 cell=floor(p),at=cell+.2+.6*vec2(hash(cell),hash(cell+7.3));
 float phase=fract(waterTime*rate+hash(cell+3.1)),radius=phase*.3;
 vec2 delta=p-at;float d=length(delta),band=d-radius;
 return delta/max(d,.001)*sin(band*60.)*exp(-band*band*900.)*(1.-phase);
}
void main(){
 vec2 p=world.xz;
 vec2 slope=vec2(cos(p.x*2.3+waterTime*1.7),sin(p.y*2.8-waterTime*1.4))*.02;
 slope+=vec2(cos(p.y*7.1+waterTime*3.),sin(p.x*6.7-waterTime*2.5))*.01;
 float foam=0.,churn=0.,plume=0.,shade=0.;
 vec3 eye=normalize(cameraPosition-world);
 for(int i=0;i<5;i++){
   vec2 delta=p-basins[i].xz;float d=length(delta);
   if(d<7.){
     vec2 out_=delta/max(d,.01);
     // The fountain's two pools (fountain-jets.js): with the tier, its pool
     // is filled to the lip, and this low disc only shows outside it.
     float poly=polygon(delta);
     bool raised=hasTier>.5&&world.y>basins[i].y+.4,low=hasTier>.5&&!raised;
     if(low&&poly<LIP)discard;
     float landing,carried,lace;
     if(hasFoam>.5){
       vec2 polar=vec2(atan(delta.y,delta.x)/6.2831853*14.,d*.85-waterTime*.22);
       lace=textureLod(foamMap,polar,0.).a*.6+textureLod(foamMap,polar*vec2(2.,1.6)+.37,0.).a*.4;
     }else lace=noise(p*5.+vec2(waterTime*.28,-waterTime*.17))*.65+noise(p*11.-waterTime*.2)*.35;
     // Patches of foam, drifting, with clear water between.
     float patches=smoothstep(.45,.7,noise(p*1.6+vec2(waterTime*.13,-waterTime*.09)));
     if(low){
       // The outer pool: white water where the overflow pours in, ripples
       // running out from that line, and the outer arcs landing farther out.
       float out2=poly-FOOT,line=exp(-pow((out2-.08)/.16,2.)),band=exp(-pow((d-5.5)/.45,2.));
       landing=line*.8+band*.6;churn=line*.7+band*.3;
       slope+=out_*sin(out2*18.-waterTime*7.5)*.03*exp(-max(out2,0.)*1.1);
       slope+=(vec2(noise(p*7.+waterTime*vec2(1.6,-1.2)),noise(p*7.3-waterTime*vec2(1.1,1.5)))-.5)*.18*(line+band*.7);
       slope+=(rings(p*3.,1.6)+rings(p*3.+.5,1.2))*.08*band;
       carried=exp(-max(out2,0.)*1.4);
       foam+=clamp(lace*(line*1.6+carried*.45+band*.7)*(.45+.55*patches)+line*.25-.2,0.,1.);
     }else{
       // The tier's pool (or the only one): churned where the umbrella lands
       // and at the jet's foot, its foam carried out to the lip and over.
       float lip=hasTier>.5?LIP:6.;
       landing=exp(-pow((d-2.5)/.9,2.))*.9+exp(-pow(d/.55,2.));churn=landing;
       slope+=(vec2(noise(p*6.+waterTime*vec2(1.3,-1.1)),noise(p*6.3-waterTime*vec2(.9,1.4)))-.5)*.22*landing;
       slope+=(vec2(noise(p*15.-waterTime*vec2(.7,1.6)),noise(p*14.+waterTime*vec2(1.5,.4)))-.5)*.12*landing;
       slope+=(rings(p*3.,1.7)+rings(p*3.+.5,1.3))*.08*landing;
       // Swell running out across the pool, quickening toward the lip.
       float waves=sin(d*6.-waterTime*4.5+noise(p*1.3+waterTime*.3)*2.5);slope+=out_*waves*.03*exp(-d*.25);
       carried=smoothstep(1.8,2.8,d)*exp(-max(d-2.8,0.)*1.2)+.6*smoothstep(lip-.5,lip,poly);
       foam+=clamp(lace*(landing*1.1+carried*.5)*(.35+.65*patches)-.22,0.,1.)*(1.-smoothstep(lip-.05,lip,poly));
     }
     // The plume's shadow: the sun's ray from here passes through the falling
     // water (a shell widening from the crown, fountain-jets.js) or the jet;
     // light through the gaps between the ropes dapples it.
     vec3 L=-waterSunDirection;
     for(int k=0;k<6;k++){
       float h=(float(k)+.5)*APEX/6.,ring=length(delta+L.xz*h/max(L.y,.2)),R=.6+1.45*sqrt(max(2.*(APEX*.9-h)/9.81,0.));
       shade+=.2*(exp(-pow((ring-R)/.45,2.))+exp(-pow(ring/(.15+.05*h),2.)));
     }
     // The plume itself, seen in the water: where the mirrored eye ray
     // passes the jet's axis, within the plume's width at that height.
     vec3 r=reflect(-eye,normalize(vec3(-slope.x*.6,1.,-slope.y*.6)));
     float along=dot(-delta,r.xz)/max(dot(r.xz,r.xz),1e-4);
     if(along>0.){
       float h=world.y-basins[i].y+r.y*along,miss=length(delta+r.xz*along),width=.9+.2*h;
       // From the outer pool the tier (its lip LIP out, standing RISE above
       // the water) hides the plume's foot.
       float wallR=hasTier>.5?LIP:2.85,wallH=hasTier>.5?RISE+.05:.6;
       float b=dot(delta,r.xz),c=d*d-wallR*wallR,disc=b*b-dot(r.xz,r.xz)*c;
       float wall=!raised&&c>0.&&disc>0.?smoothstep(wallH-.1,wallH+.15,r.y*(-b-sqrt(disc))/max(dot(r.xz,r.xz),1e-4)):1.;
       plume=max(plume,wall*(1.-smoothstep(width*.3,width,miss))*smoothstep(-.1,.4,h)*(1.-smoothstep(APEX*.85,APEX*1.12,h)));
     }
   }
 }
 for(int i=0;i<8;i++){
   float age=waterTime-wakes[i].w;
   if(age>=0.&&age<2.5&&abs(wakes[i].y-world.y)<2.){
     vec2 delta=p-wakes[i].xz;float d=length(delta),band=d-age*2.5;
     float ring=exp(-band*band*5.)*exp(-age*1.4)*motion[i].w;
     slope+=delta/max(d,.01)*cos(band*11.)*ring*.20;foam+=ring*.2;
   }
 }
 vec3 n=normalize(vec3(-slope.x,1.,-slope.y));
 float fresnel=.15+.72*pow(1.-max(0.,dot(n,eye)),3.);
 vec2 uv=gl_FragCoord.xy/waterResolution,distort=slope*.035;
 float depth=-(viewMatrix*vec4(world,1.)).z;
 float bottom=linearDepth(texture2D(sceneDepth,clamp(uv+distort,.001,.999)).r);
 if(bottom<depth)distort=vec2(0.);
 float thickness=clamp(linearDepth(texture2D(sceneDepth,uv+distort).r)-depth,0.,2.);
 vec3 transmit=texture2D(sceneColor,uv+distort).rgb;
 // Caustics on the floor, strongest in shallow, calm, sunlit water; in the
 // plume's shadow, only dapples of light through the falling water.
 shade=clamp(shade,0.,.8);
 float dapple=smoothstep(.55,.85,noise(p*7.+waterTime*vec2(2.3,-3.1)))*smoothstep(.4,.8,noise(p*3.1-waterTime*vec2(1.7,.9)));
 transmit*=1.+caustic(p*2.2+slope*3.,waterTime*.6)*1.4*exp(-thickness*1.2)*(1.-.6*churn)*clamp(-waterSunDirection.y*1.5,0.,1.)*(1.-shade);
 transmit*=1.-.4*shade+.5*shade*dapple;
 transmit=mix(transmit,vec3(.13,.29,.30),.30+(.55*(1.-exp(-thickness*.7))));
 // Air beaten into the water under the falls turns it pale.
 transmit=mix(transmit,vec3(.55,.72,.74),churn*.45);
 vec4 q=reflectionMatrix*vec4(world,1.);
 vec2 ruv=q.xy/q.w+slope*.04;
 float valid=hasReflection*step(0.,q.w)*step(0.,ruv.x)*step(ruv.x,1.)*step(0.,ruv.y)*step(ruv.y,1.);
 vec3 mirror=mix(sky,texture2D(reflection,clamp(ruv,.002,.998)).rgb,valid);
 // Screen-space reflection from the opaque copy the basin already holds: a
 // short march along the reflected ray finds the walls, rims and soldiers
 // around the basin without drawing the scene a second time.
 if(valid<.5){
   vec3 viewPos=(viewMatrix*vec4(world,1.)).xyz,viewN=normalize((viewMatrix*vec4(n,0.)).xyz);
   vec3 ray=reflect(normalize(viewPos),viewN),p=viewPos;float stride=.12,found=0.;vec3 seen=sky;
   for(int i=0;i<16;i++){
     p+=ray*stride;stride*=1.32;
     vec4 clip=projectionMatrix*vec4(p,1.);if(clip.w<=0.)break;
     vec2 at=clip.xy/clip.w*.5+.5;if(any(lessThan(at,vec2(0.)))||any(greaterThan(at,vec2(1.))))break;
     float scene=linearDepth(texture2D(sceneDepth,at).r),ray_=-p.z;
     if(ray_>scene&&ray_-scene<stride*2.+.25){
       vec2 edge=min(at,1.-at);seen=texture2D(sceneColor,at).rgb;
       found=smoothstep(0.,.08,min(edge.x,edge.y))*(1.-float(i)/16.);break;
     }
   }
   mirror=mix(sky,seen,found);
 }
 mirror=mix(mirror,min(sky*.55+waterSunColor*.55,vec3(1.)),plume*.85);
 vec3 halfVector=normalize(eye-waterSunDirection);
 float glint=pow(max(0.,dot(n,halfVector)),180.)*.65*(1.-shade);
 vec3 color=mix(transmit,mirror,fresnel)+glint*waterSunColor;
 gl_FragColor=vec4(mix(color,vec3(.88,.94,.96),clamp(foam,0.,.85)),1.);
}`;

// The basins' and tiers' meshes drawn with the water's material, each to its
// FountainWater (a renderer that draws them itself finds them here).
export const fountainWaters = new WeakMap();

// Basin water uses the opaque scene copy and analytic sky/plume reflection.
// No shadow updates, per-basin scene renders, or synchronous GL queries.
export class FountainWater {
  // tier: Theed's two-tier basins (TIER, fountain-jets.js), whose inner tier
  // Enhanced fills to its lip; the stock basin mesh is one flat disc.
  constructor(graphics,basins,wakes,foamMap=null,tier=null) {
    this.graphics=graphics;this.basins=basins;this.time=0;this.enabled=false;
    this.surface=new THREE.WebGLRenderTarget(1,1,{depthTexture:new THREE.DepthTexture(1,1,THREE.UnsignedIntType)});
    // The unported shader still declares this sampler; bind a valid empty
    // texel while its reflection comes from the screen copy and analytic sky.
    this.emptyReflection=new THREE.DataTexture(new Uint8Array(4),1,1);this.emptyReflection.needsUpdate=true;
    this.reflectionUniforms={reflection:{value:this.emptyReflection},reflectionMatrix:{value:new THREE.Matrix4()},hasReflection:{value:0}};
    // A bright overcast stand-in for the sky the basins and jets reflect; the
    // full-scene mirror re-render (20 times a second) is no longer drawn.
    this.sky=new THREE.Vector3(.7,.72,.75);
    this.material=new THREE.ShaderMaterial({vertexShader,fragmentShader,transparent:true,depthWrite:false,side:THREE.DoubleSide,
      uniforms:{...this.reflectionUniforms,sky:{value:this.sky},waterTime:{value:0},sceneColor:{value:this.surface.texture},sceneDepth:{value:this.surface.depthTexture},
        foamMap:{value:foamMap},hasFoam:{value:foamMap?1:0},hasTier:{value:tier?1:0},waterSunDirection:lighting.sunDirection,waterSunColor:lighting.sunColor,
        waterResolution:{value:new THREE.Vector2()},cameraRange:{value:new THREE.Vector2()},wakes:{value:wakes.points},motion:{value:wakes.motion},
        basins:{value:Array.from({length:5},(_,i)=>new THREE.Vector4(...(basins[i]?.center||[1e6,1e6,1e6]),basins[i]?.radius||0))}},toneMapped:false});
    this.original=[];
    graphics.scene.traverse(mesh=>{if(mesh.isMesh&&basins.some(b=>b.texture===mesh.name))
      this.original.push({mesh,material:mesh.material,order:mesh.renderOrder,before:mesh.onBeforeRender});});
    // The tiers' pools, filled to their lips: 12-sided discs, one draw for all.
    if(tier){
      const position=[],index=[];
      for(const b of basins){
        const base=position.length/3,[x,y,z]=b.center;position.push(x,y+tier.rise,z);
        for(let k=0;k<=tier.sides;k++){const a=k/tier.sides*Math.PI*2;position.push(x+Math.cos(a)*tier.lip,y+tier.rise,z+Math.sin(a)*tier.lip);}
        for(let k=1;k<=tier.sides;k++)index.push(base,base+k+1,base+k);
      }
      const geometry=new THREE.BufferGeometry();geometry.setAttribute('position',new THREE.Float32BufferAttribute(position,3));geometry.setIndex(index);
      this.tier=new THREE.Mesh(geometry,this.material);this.tier.name='fountain-tier-water';this.tier.renderOrder=-16.9;
      this.tier.frustumCulled=false;this.tier.visible=false;this.tier.onBeforeRender=(renderer,scene,camera)=>this.capture(renderer,camera);
      graphics.scene.add(this.tier);fountainWaters.set(this.tier,this);
    }
  }
// This setting changes the basin's authored material, not its drawer.
// The original material and hooks are retained for the disabled appearance.
// Whichever appearance is selected still has to be claimed by Rust before
// the near scene can draw; changing water quality cannot enable a native pass.
// Tier pools share that material and participate in the same ordered capture.
  setEnabled(enabled){
    if(enabled===this.enabled)return;this.enabled=enabled;this.reflectionUniforms.hasReflection.value=0;
    for(const o of this.original){o.mesh.material=enabled?this.material:o.material;o.mesh.renderOrder=enabled?-17:o.order;
      o.mesh.onBeforeRender=enabled?(renderer,scene,camera)=>this.capture(renderer,camera):o.before;
      if(enabled)fountainWaters.set(o.mesh,this);else fountainWaters.delete(o.mesh);}
    if(this.tier)this.tier.visible=enabled;
  }
  update(time){this.time=time;this.material.uniforms.waterTime.value=time;this.captured=false;
    const top=lighting.ambientTop.value;this.sky.set(Math.min(1,top.x*1.4+.15),Math.min(1,top.y*1.4+.16),Math.min(1,top.z*1.4+.2));}
  // The composite samples the opaque colour/depth at its ordered boundary.
  // Copy only once for this frame and only from the main scene target; mirrors
  // have their own inputs and cannot reuse a partially drawn main surface.
  // Target binding is still owned by Three, while the eventual composite draw
  // belongs to the required Rust transparent pass.
  capture(renderer,camera){
    const source=renderer.getRenderTarget();if(this.captured||source!==this.graphics.sceneTarget)return;
    this.captured=true;this.surface.setSize(source.width,source.height);
    const gl=renderer.getContext(),framebuffer=drawFramebuffer(renderer,source);
    renderer.setRenderTarget(this.surface);renderer.state.bindFramebuffer(gl.READ_FRAMEBUFFER,null);
    renderer.state.bindFramebuffer(gl.READ_FRAMEBUFFER,framebuffer);
    gl.blitFramebuffer(0,0,source.width,source.height,0,0,source.width,source.height,gl.COLOR_BUFFER_BIT|gl.DEPTH_BUFFER_BIT,gl.NEAREST);
    renderer.setRenderTarget(source);this.material.uniforms.waterResolution.value.set(source.width,source.height);
    this.material.uniforms.cameraRange.value.set(camera.near,camera.far);
  }
  // Start another frame's opaque copy. Basin reflection samples that copy;
  // its analytic surface has no separate planar scene camera.
  reflect() {this.captured=false;this.reflectionUniforms.hasReflection.value=0;}
  // The meshes drawn with the water's material now (the basins' while it is
  // on, the tiers' pools).
  meshes(){return [...(this.enabled?this.original.map((o)=>o.mesh):[]),...(this.tier?[this.tier]:[])];}
  dispose(){this.setEnabled(false);this.tier?.removeFromParent();this.tier?.geometry.dispose();this.material.dispose();this.emptyReflection.dispose();this.surface.dispose();}
}
