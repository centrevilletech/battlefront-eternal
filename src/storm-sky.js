import * as THREE from 'three';
import { lighting } from './materials.js';
import { fullScreenPass } from './render-passes.js';
import { SKY_ORDER } from './world.js';

// A world-oriented panorama means looking around never invalidates the cloud
// cache. Only the low-resolution volume integration runs at 10 Hz; lightning,
// camera rotation and final composition run at the display's frame rate.
const CLOUDS = /* glsl */`
 uniform sampler3D volumeNoise;
 uniform vec2 wind;
 uniform vec3 eyePosition,hazeColor;
 uniform float time;
 varying vec2 vUv;
 float field(vec3 p){
   p.xz-=wind*time*7.0;
   vec3 q=p*.000065;
   float shape=texture(volumeNoise,q).r*.58+texture(volumeNoise,q*2.03).r*.28+texture(volumeNoise,q*4.11).r*.14;
   float height=(p.y-180.0)/460.0;
   float envelope=smoothstep(0.0,.18,height)*(1.0-smoothstep(.65,1.0,height));
   return max(0.0,shape-.48)*envelope*5.5;
 }
 void main(){
   float azimuth=(vUv.x-.5)*6.2831853,elevation=vUv.y*1.5707963;
   vec3 ray=vec3(sin(azimuth)*cos(elevation),sin(elevation),cos(azimuth)*cos(elevation));
   if(ray.y<.015){gl_FragColor=vec4(hazeColor,0);return;}
   float start=max(0.0,(180.0-eyePosition.y)/ray.y),end=max(0.0,(640.0-eyePosition.y)/ray.y);
   end=min(end,11000.0);
   float stepSize=max(1.0,(end-start)/28.0),transmission=1.0;vec3 color=vec3(0);
   // Fixed per-ray jitter removes marching bands without temporal grain.
   float jitter=fract(sin(dot(gl_FragCoord.xy,vec2(12.9898,78.233)))*43758.5453);
   for(int i=0;i<28;i++){
     vec3 p=eyePosition+ray*(start+(float(i)+jitter)*stepSize);
     float density=field(p);
     if(density>.005){
       float light=exp(-field(p+vec3(-80,130,60))*2.7);
       float top=clamp((p.y-180.0)/460.0,0.0,1.0);
       vec3 cloud=mix(vec3(.012,.018,.03),vec3(.26,.32,.39),pow(light,1.7)*.72+top*.18);
       float opacity=1.0-exp(-density*stepSize*.022);
       color+=transmission*opacity*cloud;transmission*=1.0-opacity;
       if(transmission<.015)break;
     }
   }
   vec3 sky=mix(hazeColor,vec3(.028,.042,.065),smoothstep(.0,.7,ray.y));
   color+=transmission*sky;
   float fog=1.0-exp(-start*.00014);
   color=mix(color,hazeColor,fog);
   gl_FragColor=vec4(color,1.0-transmission);
 }
`;
const SKY = /* glsl */`
 uniform sampler2D cloudMap;
 uniform mat4 viewRotation,inverseProjection;
 uniform vec3 eyePosition,lightningPosition;
 uniform float lightningFlash;
 varying vec2 vUv;
 void main(){
   vec4 view=inverseProjection*vec4(vUv*2.0-1.0,1,1);
   vec3 ray=normalize(mat3(viewRotation)*view.xyz);
   vec2 uv=vec2(atan(ray.x,ray.z)/6.2831853+.5,asin(clamp(ray.y,0.0,1.0))/1.5707963);
   vec4 cloud=texture2D(cloudMap,uv);
   vec3 strike=normalize(lightningPosition-eyePosition);
   float glow=pow(max(dot(ray,strike),0.0),38.0);
   vec3 color=cloud.rgb+vec3(.52,.64,.8)*lightningFlash*(.055+glow*(.35+cloud.a*.7));
   gl_FragColor=vec4(color,1);
 }
`;
const BOLT_VERTEX=/* glsl */`
 attribute vec3 tangent;
 attribute float edge,width,energy;
 varying float vEdge,vEnergy;
 void main(){
   vec3 across=normalize(cross(tangent,normalize(cameraPosition-position)));
   vec3 world=position+across*edge*width;
   vEdge=edge;vEnergy=energy;gl_Position=projectionMatrix*viewMatrix*vec4(world,1);
 }
`;
const BOLT_FRAGMENT=/* glsl */`
 uniform float lightningFlash;
 varying float vEdge,vEnergy;
 void main(){float a=pow(max(0.0,1.0-abs(vEdge)),1.5)*lightningFlash*vEnergy;
   gl_FragColor=vec4(.72,.84,1.0,a);}
`;
// The models on the dome's horizon (Kamino's city) in the storm, in place of
// their sky material: the texture as the sky draws it, in the storm's dim,
// cool, grey light (the light painted into the texture loses some of its
// colour; lit windows keep their glow), seen through its sea haze, and lit by
// a flash as the clouds are, most on the strike's bearing. Below the horizon
// the ocean covers them, and the horizon veil (ocean-storm.js) thickens the
// haze toward their feet.
const HORIZON_VERTEX=/* glsl */`
 varying vec2 vUv;
 varying vec3 vRay;
 void main(){
   vUv=uv;vec4 world=modelMatrix*vec4(position,1);vRay=world.xyz-cameraPosition;
   gl_Position=projectionMatrix*viewMatrix*world;
 }
`;
const HORIZON_FRAGMENT=/* glsl */`
 uniform sampler2D map;
 uniform vec3 hazeColor,lightningPosition;
 uniform float lightningFlash;
 varying vec2 vUv;
 varying vec3 vRay;
 void main(){
   vec4 texel=texture2D(map,vUv);
   #ifdef ALPHA_TEST
     if(texel.a<.5)discard;
   #endif
   float luma=dot(texel.rgb,vec3(.3,.59,.11));
   vec3 color=mix(texel.rgb,vec3(luma),.4)*vec3(.64,.74,.88)+texel.rgb*smoothstep(.5,.9,luma)*.7;
   vec2 strike=lightningPosition.xz-cameraPosition.xz;
   float bearing=max(dot(vRay.xz,strike),0.0)/max(length(vRay.xz)*length(strike),1e-4);
   float glow=pow(bearing,4.0);
   vec3 flash=vec3(.52,.64,.8)*lightningFlash;
   color+=texel.rgb*flash*(.3+glow*1.2);
   #ifdef ADDITIVE
     color*=1.0-.24;
   #else
     color=mix(color,hazeColor+flash*(.055+glow*.35),.24);
   #endif
   #ifdef BLENDED
     gl_FragColor=vec4(color,texel.a);
   #else
     gl_FragColor=vec4(color,1.0);
   #endif
 }
`;

export class StormSky {
 constructor({graphics,uniforms}) {
   this.graphics=graphics;this.uniforms=uniforms;this.enabled=false;this.lastCloud=-Infinity;
   this.nextStrike=6;this.strikeStart=-100;this.strikes=0;this.randomState=8217;
   this.lastEye=new THREE.Vector3(Infinity,Infinity,Infinity);
   this.lightKeys=['ambient','ambientTop','ambientBottom','characterAmbient'];
   this.lightBase=this.lightKeys.map(key=>lighting[key].value.clone());this.previousFlash=0;
   const bytes=new Uint8Array(64**3);
   for(let i=0;i<bytes.length;i++)bytes[i]=this.random()*255;
   this.noise=new THREE.Data3DTexture(bytes,64,64,64);
   this.noise.format=THREE.RedFormat;this.noise.minFilter=this.noise.magFilter=THREE.LinearFilter;
   this.noise.wrapS=this.noise.wrapT=this.noise.wrapR=THREE.RepeatWrapping;this.noise.unpackAlignment=1;this.noise.needsUpdate=true;
   this.target=new THREE.WebGLRenderTarget(768,384,{depthBuffer:false});
   this.target.texture.wrapS=THREE.RepeatWrapping;
   Object.assign(uniforms,{cloudMap:{value:this.target.texture},cloudReady:{value:0},volumeNoise:{value:this.noise},eyePosition:{value:new THREE.Vector3()},
     lightningFlash:{value:0},lightningPosition:{value:new THREE.Vector3()},viewRotation:{value:new THREE.Matrix4()},inverseProjection:{value:new THREE.Matrix4()}});
   this.cloudPass=fullScreenPass(CLOUDS,uniforms);this.passCamera=new THREE.Camera();
   // Drawn in the dome's place, just after it (world.js), so the models on
   // its horizon draw over the panorama as they draw over the dome.
   this.sky=fullScreenPass(SKY,uniforms);this.sky.name='storm-volume-clouds';this.sky.renderOrder=SKY_ORDER+.5;
   this.horizonMaterials=new Map();this.replaced=[];
   this.sky.onBeforeRender=(renderer,scene,camera)=>{uniforms.viewRotation.value.copy(camera.matrixWorld);uniforms.inverseProjection.value.copy(camera.projectionMatrixInverse);};
   this.boltGeometry=new THREE.BufferGeometry();
   this.positions=new Float32Array(1536*3);this.tangents=new Float32Array(1536*3);
   this.edges=new Float32Array(1536);this.widths=new Float32Array(1536);this.energies=new Float32Array(1536);
   for(const [name,array,itemSize] of [['position',this.positions,3],['tangent',this.tangents,3],['edge',this.edges,1],['width',this.widths,1],['energy',this.energies,1]])
     this.boltGeometry.setAttribute(name,new THREE.BufferAttribute(array,itemSize).setUsage(THREE.DynamicDrawUsage));
   this.boltGeometry.setDrawRange(0,0);
   this.bolt=new THREE.Mesh(this.boltGeometry,new THREE.ShaderMaterial({uniforms,vertexShader:BOLT_VERTEX,fragmentShader:BOLT_FRAGMENT,transparent:true,depthWrite:false,blending:THREE.AdditiveBlending,side:THREE.DoubleSide,forceSinglePass:true}));
   this.bolt.name='storm-lightning';this.bolt.frustumCulled=false;this.bolt.renderOrder=8;
   this.sky.visible=this.bolt.visible=false;graphics.farScene.add(this.sky,this.bolt);
 }
 random(){this.randomState=(Math.imul(this.randomState,1664525)+1013904223)>>>0;return this.randomState/4294967296;}
 setEnabled(enabled) {
   if(this.enabled===enabled)return;
   this.enabled=enabled;this.sky.visible=enabled;this.bolt.visible=false;
   if(enabled){
     // Every part of the dome gives way to the panorama; the models on its
     // horizon stay, in the storm's light.
     const graphics=this.graphics,layers=graphics.skyLayers?.length?graphics.skyLayers:graphics.sky?[{mesh:graphics.sky,layer:0}]:[];
     this.replaced=layers.map(({mesh,layer})=>({mesh,layer,visible:mesh.visible,material:mesh.material}));
     for(const {mesh,layer,material} of this.replaced){if(layer)mesh.material=this.horizonMaterial(material);else mesh.visible=false;}
     this.lastCloud=-Infinity;
   } else {
     for(const {mesh,layer,visible,material} of this.replaced){if(layer)mesh.material=material;else mesh.visible=visible;}
     this.replaced=[];this.restoreLight();this.uniforms.lightningFlash.value=0;
   }
 }
 // One storm material for each sky material a horizon model draws with, made
 // once and kept through mode switches. It blends as the sky material does.
 horizonMaterial(original) {
   let material=this.horizonMaterials.get(original);
   if(material)return material;
   const defines={};
   for(const key of ['ALPHA_TEST','BLENDED','ADDITIVE'])if(key in (original.defines||{}))defines[key]='';
   material=new THREE.ShaderMaterial({name:'storm-horizon',defines,vertexShader:HORIZON_VERTEX,fragmentShader:HORIZON_FRAGMENT,
     uniforms:{map:original.uniforms?.map||{value:original.map||null},hazeColor:this.uniforms.hazeColor,
       lightningFlash:this.uniforms.lightningFlash,lightningPosition:this.uniforms.lightningPosition},
     transparent:false,depthTest:false,depthWrite:false,side:original.side,alphaToCoverage:original.alphaToCoverage,
     blending:original.blending,blendSrc:original.blendSrc,blendDst:original.blendDst,blendEquation:original.blendEquation});
   this.horizonMaterials.set(original,material);
   return material;
 }
 restoreLight(){
   if(!this.previousFlash)return;
   for(let i=0;i<this.lightKeys.length;i++)lighting[this.lightKeys[i]].value.copy(this.lightBase[i]);
   this.previousFlash=0;
 }
 strike(time,camera) {
   this.strikeStart=time;this.nextStrike=time+7+this.random()*11;this.strikes++;
   // Distributed around the storm, never locked to the player's reticle.
   const angle=this.random()*Math.PI*2,distance=430+this.random()*220;
   const x=camera.position.x+Math.cos(angle)*distance,z=camera.position.z+Math.sin(angle)*distance;
   const top=new THREE.Vector3(x,300+this.random()*100,z);this.uniforms.lightningPosition.value.copy(top);
   let count=0;
   const segment=(a,b,width,energy)=>{
     for(const layer of [1,4])for(const [end,edge] of [[0,-1],[1,-1],[0,1],[0,1],[1,-1],[1,1]]){
       const p=end?b:a;this.positions.set([p.x,p.y,p.z],count*3);this.tangents.set([b.x-a.x,b.y-a.y,b.z-a.z],count*3);
       this.edges[count]=edge;this.widths[count]=width*layer;this.energies[count]=energy*(layer===1?1:.16);count++;
     }
   };
   let a=top.clone();
   const steps=28,bottom=-8;
   for(let i=1;i<=steps;i++){
     const b=new THREE.Vector3(a.x+(this.random()-.5)*22,top.y+(bottom-top.y)*i/steps,a.z+(this.random()-.5)*22);
     segment(a,b,.7,1);
     if(i>3&&i<22&&i%5===0){
       let branch=a.clone();const dx=(this.random()-.5)*30,dz=(this.random()-.5)*30;
       for(let j=0;j<6;j++){const tip=new THREE.Vector3(branch.x+dx+(this.random()-.5)*10,branch.y-10-this.random()*12,branch.z+dz);segment(branch,tip,.45,(1-j/7)*.65);branch=tip;}
     }
     a=b;
   }
   this.boltGeometry.setDrawRange(0,count);for(const attr of Object.values(this.boltGeometry.attributes))attr.needsUpdate=true;
 }
 update(time,camera) {
   if(!this.enabled)return;
   this.restoreLight();
   if(time>=this.nextStrike)this.strike(time,camera);
   const age=time-this.strikeStart;
   const pulse=(start,span,power)=>age<start||age>start+span?0:power*Math.sin(Math.PI*(age-start)/span);
   const flash=pulse(0,.12,1)+pulse(.19,.08,.6)+pulse(.34,.19,.8);
   this.uniforms.lightningFlash.value=flash;this.bolt.visible=flash>.005;
   if(flash>0){
     for(let i=0;i<this.lightKeys.length;i++){
       const value=lighting[this.lightKeys[i]].value;this.lightBase[i].copy(value);
       value.x+=flash*.10;value.y+=flash*.13;value.z+=flash*.18;
     }
     this.previousFlash=flash;
   }
   this.uniforms.eyePosition.value.copy(camera.position);
   const renderer=this.graphics.renderer;
   if(!renderer||time-this.lastCloud<.1&&this.lastEye.distanceToSquared(camera.position)<256)return;
   this.lastCloud=time;this.lastEye.copy(camera.position);this.cloudRenders=(this.cloudRenders||0)+1;
   const target=renderer.getRenderTarget(),autoClear=renderer.autoClear,shadowAuto=renderer.shadowMap.autoUpdate;
   try{
     renderer.shadowMap.autoUpdate=false;renderer.autoClear=false;
     renderer.setRenderTarget(this.target);renderer.render(this.cloudPass,this.passCamera);
     this.uniforms.cloudReady.value=1;
   } finally {renderer.setRenderTarget(target);renderer.autoClear=autoClear;renderer.shadowMap.autoUpdate=shadowAuto;}
 }
 dispose(){this.setEnabled(false);this.sky.removeFromParent();this.bolt.removeFromParent();this.sky.material.dispose();this.cloudPass.material.dispose();this.boltGeometry.dispose();this.bolt.material.dispose();this.target.dispose();this.noise.dispose();
   for(const material of this.horizonMaterials.values())material.dispose();this.horizonMaterials.clear();}
}
