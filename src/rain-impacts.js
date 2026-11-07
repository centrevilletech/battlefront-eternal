import { EFFECT } from './rust-render/effect-layout.js';
import * as THREE from 'three';
import { particleDepth } from './particle-depth.js';
const COUNT=96;
export const rainImpacts=new WeakMap();
// RainImpacts keeps weather.rs's persistent draw rows. Geometry, the two
// repeated splash kinds and material policy are native renderer resources.
export class RainImpacts {
  constructor({scene,locations,normals}) {
    this.next=0;this.queries=0;this.locations=locations;this.normals=normals;
    const kinds=new Float32Array(COUNT*2);for(let i=0;i<kinds.length;i++)kinds[i]=i%2;
    this.nativeKind=EFFECT.IMPACTS.ID;this.family='impacts';this.variant=0;this.texture=null;
    this.capacity=COUNT*2;this.count=COUNT*2;this.arrays={impact:locations,surfaceNormal:normals,kind:kinds};
    this.values=new Float64Array(EFFECT.IMPACTS.LENGTH);this.visibility='anchor';
    this.mesh=new THREE.Object3D();this.mesh.userData.effectBatch=this;
    this.mesh.name='rain-surface-impacts';this.mesh.frustumCulled=false;this.mesh.renderOrder=21;this.mesh.visible=false;
    this.shown=false;this.serial=0;rainImpacts.set(this.mesh,this);
    this.mesh.onBeforeRender=(r,s,c)=>particleDepth.capture(r,s,c);scene.add(this.mesh);
  }
  // No BufferAttribute dirty ranges are needed: the effects phase copies the
  // active native rows. Visibility and the clock remain weather facts.
  synced(time,enabled,changed,next,queries) {
    this.values[EFFECT.IMPACTS.IMPACT_TIME]=time;this.shown=enabled;this.mesh.visible=enabled;
    this.next=next;this.queries=queries;if(changed)this.serial++;
  }
  dispose(){this.mesh.removeFromParent();}
}
