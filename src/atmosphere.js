import {sceneProperty} from './rust-render/scene-changes.js';
import {EFFECT} from './rust-render/effect-layout.js';
import * as THREE from 'three';
import {makeGround} from './collision.js';
import {AtmosphereHost,drawValues} from './environment/module.js';
import {AtmosphereStats as STATS,SquareBatch as SQUARE,Squares,View as VIEW,EnvironmentDraw as DRAW} from './environment/layout.js';
import {particleDepth} from './particle-depth.js';

// Scene and texture identities stay with WebGL. Native admission resolves fog
// and dust classes; the phase retains their clocks, geometry and draw values.
export const squareBatches=new WeakMap();
class SquareBatch {
  constructor(scene,texture,name,host,index) {
    this.nativeKind=EFFECT.SQUARE.ID;this.variant=0;this.texture=texture;
    this.capacity=Squares.CAPACITY;this.count=0;this.arrays={};
    drawValues(host,this,DRAW.SQUARE,EFFECT.SQUARE,index);
    host.bindArray(this.arrays,'center',h=>h.batches[index].center);
    host.bindArray(this.arrays,'spin',h=>h.batches[index].spin);
    this.mesh=new THREE.Object3D();this.mesh.name=name;
    sceneProperty(this.mesh,'frustumCulled',false);sceneProperty(this.mesh,'visible',false);sceneProperty(this.mesh,'renderOrder',10);
    this.mesh.onBeforeRender=(r,s,c)=>particleDepth.capture(r,s,c);scene.add(this.mesh);
    let root=scene;while(root.parent)root=root.parent;
    if(!squareBatches.has(root))squareBatches.set(root,[]);
    squareBatches.get(root).push(this);
  }
  dispose(){this.mesh.removeFromParent();}
}
export class Atmosphere {
  constructor({data,scene,collision,texture,surfaceOwner}) {
    const maps=new Map(),available={};
    for(const [name,def]of Object.entries(data?.dust?.classes||{})){const map=texture(def.texture);maps.set(def.texture,map);available[name]=Boolean(map);}
    const fogMap=data?.fogCloud&&texture(data.fogCloud.texture);
    if(fogMap)maps.set(data.fogCloud.texture,fogMap);
    const ground=makeGround();
    this.host=new AtmosphereHost(surfaceOwner,0,0,false,(x,z)=>collision?.ground(x,z,100000,ground)?.height??NaN,
      {data:{fogCloud:data?.fogCloud,dust:{classes:Object.entries(data?.dust?.classes||{}),instances:data?.dust?.instances||[]}},available,fog:Boolean(fogMap)});
    this.batches=this.host.recipe.batches.map((b,k)=>new SquareBatch(scene,maps.get(b.texture),b.name,this.host,k));
    this.stats={fog:0,dust:0,objects:0};
  }
  // Camera matrices are external observations, not reconstructed transforms.
  prepareFrame(dt,camera) {
    this.host.refresh();const v=this.host.view;v[VIEW.DT]=dt;
    v.set(camera.matrixWorld.elements,VIEW.CAMERA);v.set(camera.matrixWorldInverse.elements,VIEW.INVERSE);v.set(camera.projectionMatrix.elements,VIEW.PROJECTION);
    v[VIEW.FOV]=camera.fov??NaN;v[VIEW.ASPECT]=camera.aspect??NaN;
  }
  publishFrame() {
    this.host.refresh();const r=this.host.result;
    this.stats.fog=r[STATS.FOG];this.stats.dust=r[STATS.DUST];this.stats.objects=r[STATS.OBJECTS];
    for(let k=0;k<this.batches.length;k++){const b=this.batches[k],at=STATS.STRIDE+k*SQUARE.STRIDE;b.count=r[at+SQUARE.COUNT];sceneProperty(b.mesh,'visible',Boolean(r[at+SQUARE.VISIBLE]));}
  }
  dispose(){for(const batch of this.batches)batch.dispose();this.batches.length=0;}
}
