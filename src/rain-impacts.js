import { ownedEffectMaterial } from './rust-render/effect-material.js';
import * as THREE from 'three';
import {particleDepth} from './particle-depth.js';
const COUNT=96;
// Each splash mesh's RainImpacts, for a renderer that draws the splashes
// itself (src/rust-render/effects.js).
export const rainImpacts=new WeakMap();
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
  const material=ownedEffectMaterial('effects/rain-impacts', {uniforms:{impactTime:{value:0},sceneDepth:particleDepth.map,softParticles:particleDepth.enabled,depthResolution:particleDepth.resolution,cameraRange:particleDepth.cameraRange},transparent:true,depthWrite:false,side:THREE.DoubleSide,forceSinglePass:true});
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
