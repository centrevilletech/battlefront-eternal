import * as THREE from 'three';
import {particleDepth} from './particle-depth.js';
const COUNT=96,LIFE=.65;
// Each splash mesh's RainImpacts, for a renderer that draws the splashes
// itself (src/rust-render/effects.js).
export const rainImpacts=new WeakMap();
const VERTEX=/* glsl */`
 attribute vec4 impact;
 attribute vec3 surfaceNormal;
 attribute float kind;
 uniform float impactTime;
 varying vec2 vUv;
 varying float vAge,vKind,vDepth;
 void main(){
  float age=(impactTime-impact.w)/${LIFE};vAge=age;vKind=kind;vUv=uv;
  vec3 N=normalize(surfaceNormal),T=normalize(cross(N,abs(N.y)>.95?vec3(1,0,0):vec3(0,1,0))),B=cross(N,T);
  vec3 world=impact.xyz+N*.025;
  if(kind<.5){world+=(T*position.x+B*position.y)*(.035+age*.30);}
  vec4 view=viewMatrix*vec4(world,1.0);
  if(kind>.5){
   float rise=max(0.0,age*.65-age*age*.7);
   view.xyz+=(mat3(viewMatrix)*N)*rise;
   view.xy+=position.xy*vec2(.08+age*.2,.13+age*.08);
  }
  vDepth=-view.z;
  gl_Position=projectionMatrix*view;
  if(age<0.0||age>1.0)gl_Position=vec4(2,2,2,1);
 }
`;
const FRAGMENT=/* glsl */`
 uniform sampler2D sceneDepth;
 uniform float softParticles;
 uniform vec2 depthResolution,cameraRange;
 varying vec2 vUv;
 varying float vAge,vKind,vDepth;
 float hash(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}
 void main(){
  vec2 q=vUv*2.0-1.0;float a;
  if(vKind<.5){float r=length(q);a=exp(-pow((r-.72)*20.0,2.0))*(1.0-vAge)*.32;}
  else{
   vec2 cell=vec2(9,6);vec2 id=floor(vUv*cell),p=fract(vUv*cell)-.5;
   float droplet=1.0-smoothstep(.08,.3,length(p*vec2(1,1.8)));
   a=droplet*step(.48,hash(id))*pow(1.0-vAge,2.0)*.55*(1.0-smoothstep(.5,1.0,length(q)));
  }
  if(softParticles>.5){float d=texture2D(sceneDepth,gl_FragCoord.xy/depthResolution).r;
   float z=cameraRange.x*cameraRange.y/(cameraRange.y-d*(cameraRange.y-cameraRange.x));
   a*=clamp((z-vDepth)/.06,0.0,1.0);}
  if(a<.002)discard;
  gl_FragColor=vec4(.72,.81,.9,a);
 }
`;
// The splashes' mesh and material; their records, their clock and what lands
// where are the Rust phase's (rust/bfe-environment, weather.rs): this keeps
// what three or the renderer needs of them.
// The splashes' mesh and material. Their records, their clock and what lands where
// are the Rust phase's; this keeps what three, or the renderer that draws them
// itself, needs of them.
export class RainImpacts{
 constructor({scene,locations,normals}){
  this.next=0;this.queries=0;this.locations=locations;this.normals=normals;
  const base=new THREE.PlaneGeometry(2,2),g=new THREE.InstancedBufferGeometry();g.setIndex(base.index.clone());
  g.setAttribute('position',base.attributes.position.clone());g.setAttribute('uv',base.attributes.uv.clone());base.dispose();
  // Each record is drawn twice: kind 0 a ring that spreads, kind 1 a droplet that
  // rises and falls.
  const kinds=new Float32Array(COUNT*2);for(let i=0;i<COUNT*2;i++)kinds[i]=i%2;
  g.setAttribute('impact',new THREE.InstancedBufferAttribute(locations,4).setUsage(THREE.DynamicDrawUsage));
  g.setAttribute('surfaceNormal',new THREE.InstancedBufferAttribute(normals,3).setUsage(THREE.DynamicDrawUsage));
  g.setAttribute('kind',new THREE.InstancedBufferAttribute(kinds,1));g.instanceCount=COUNT*2;
  const material=new THREE.ShaderMaterial({vertexShader:VERTEX,fragmentShader:FRAGMENT,uniforms:{impactTime:{value:0},sceneDepth:particleDepth.map,softParticles:particleDepth.enabled,depthResolution:particleDepth.resolution,cameraRange:particleDepth.cameraRange},transparent:true,depthWrite:false,side:THREE.DoubleSide,forceSinglePass:true});
  this.mesh=new THREE.Mesh(g,material);this.mesh.name='rain-surface-impacts';this.mesh.frustumCulled=false;this.mesh.renderOrder=21;this.mesh.visible=false;
  // Whether the splashes show this frame; while `external` another renderer
  // draws them (src/rust-render/effects.js) and three is shown none.
  this.shown=false;this.external=false;rainImpacts.set(this.mesh,this);
  this.mesh.onBeforeRender=(r,s,c)=>particleDepth.capture(r,s,c);scene.add(this.mesh);
 }
 // After the phase's frame: its clock, whether they show, and whether any
 // landed (marked for three to upload).
 // After the phase's frame: its clock, whether the splashes show, and whether any
 // landed (marked for three to upload; a renderer that draws them copies them
 // itself).
 synced(time,enabled,changed,next,queries){
  this.mesh.material.uniforms.impactTime.value=time;this.shown=enabled;this.mesh.visible=enabled&&!this.external;this.next=next;this.queries=queries;
  if(changed){this.mesh.geometry.attributes.impact.needsUpdate=true;this.mesh.geometry.attributes.surfaceNormal.needsUpdate=true;}
 }
 dispose(){this.mesh.removeFromParent();this.mesh.geometry.dispose();this.mesh.material.dispose();}
}
