import * as THREE from 'three';
import {makeHit} from './collision.js';
import {particleDepth} from './particle-depth.js';
const DOWN=new THREE.Vector3(0,-1,0),COUNT=96,LIFE=.65;
const VERTEX=/* glsl */`
 attribute vec4 impact;
 attribute vec3 surfaceNormal;
 attribute float kind;
 uniform float time;
 varying vec2 vUv;
 varying float vAge,vKind,vDepth;
 void main(){
  float age=(time-impact.w)/${LIFE};vAge=age;vKind=kind;vUv=uv;
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
export class RainImpacts {
 constructor({scene,collision,water}){
  Object.assign(this,{collision,water});this.time=0;this.acc=0;this.next=0;this.queries=0;this.randomState=19;
  this.from=new THREE.Vector3();this.hit=makeHit();
  const base=new THREE.PlaneGeometry(2,2),g=new THREE.InstancedBufferGeometry();g.setIndex(base.index.clone());
  g.setAttribute('position',base.attributes.position.clone());g.setAttribute('uv',base.attributes.uv.clone());base.dispose();
  this.locations=new Float32Array(COUNT*2*4);this.normals=new Float32Array(COUNT*2*3);const kinds=new Float32Array(COUNT*2);
  for(let i=0;i<COUNT*2;i++){this.locations[i*4+3]=-100;this.normals[i*3+1]=1;kinds[i]=i%2;}
  g.setAttribute('impact',new THREE.InstancedBufferAttribute(this.locations,4).setUsage(THREE.DynamicDrawUsage));
  g.setAttribute('surfaceNormal',new THREE.InstancedBufferAttribute(this.normals,3).setUsage(THREE.DynamicDrawUsage));
  g.setAttribute('kind',new THREE.InstancedBufferAttribute(kinds,1));g.instanceCount=COUNT*2;
  const material=new THREE.ShaderMaterial({vertexShader:VERTEX,fragmentShader:FRAGMENT,uniforms:{time:{value:0},sceneDepth:particleDepth.map,softParticles:particleDepth.enabled,depthResolution:particleDepth.resolution,cameraRange:particleDepth.cameraRange},transparent:true,depthWrite:false,side:THREE.DoubleSide,forceSinglePass:true});
  this.mesh=new THREE.Mesh(g,material);this.mesh.name='rain-surface-impacts';this.mesh.frustumCulled=false;this.mesh.renderOrder=21;this.mesh.visible=false;
  this.mesh.onBeforeRender=(...args)=>particleDepth.capture(...args);scene.add(this.mesh);
 }
 random(){this.randomState=Math.imul(this.randomState,1664525)+1013904223>>>0;return this.randomState/4294967296;}
 update(dt,camera,enabled){
  this.time+=dt;this.mesh.material.uniforms.time.value=this.time;this.mesh.visible=enabled;this.queries=0;
  if(!enabled){this.acc=0;return;}
  this.acc=Math.min(4,this.acc+dt*90);let changed=false;
  while(this.acc>=1){
   this.acc--;this.queries++;
   const radius=Math.sqrt(this.random())*11,angle=this.random()*Math.PI*2;
   this.from.set(camera.position.x+Math.cos(angle)*radius,camera.position.y+128,camera.position.z+Math.sin(angle)*radius);
   const hit=this.collision.cast(this.from,DOWN,180,this.hit);
   let y=hit?.point.y??-Infinity;const normal=hit?.normal;
   const water=this.water?.over(this.from.x,this.from.z)&&this.water.level>y;
   if(water)y=this.water.level;
   if(!Number.isFinite(y)||y>camera.position.y+1||y<camera.position.y-12||(!water&&normal.y<.6))continue;
   for(let kind=0;kind<2;kind++){
    const index=this.next*2+kind;
    this.locations.set([this.from.x,y,this.from.z,this.time],index*4);
    this.normals.set(water?[0,1,0]:[normal.x,normal.y,normal.z],index*3);
   }
   this.next=(this.next+1)%COUNT;changed=true;
  }
  if(changed){this.mesh.geometry.attributes.impact.needsUpdate=true;this.mesh.geometry.attributes.surfaceNormal.needsUpdate=true;}
 }
 dispose(){this.mesh.removeFromParent();this.mesh.geometry.dispose();this.mesh.material.dispose();}
}
