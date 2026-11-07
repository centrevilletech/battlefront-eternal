// Interface scenes publish native source/appearance handles plus live UI state.
// Geometry, groups, shader/raster/sampler descriptors and uniform packing are
// retained in Rust. Scene selection and matrices keep their existing producer.
import * as THREE from 'three';
import {UI} from './interface-layout.js';
import {UI_RESOURCE as U} from './interface-resource-layout.js';
import {nativeActorSource,actorSkin,publishActorSkin} from '../actor-resources.js';
import {imageSample} from '../interface/images.js';
export class InterfacePass {
 constructor(core){this.core=core;this.nodes=new WeakMap();this.rows=[];this.count=0;this.frame=0;this.skeletons=new WeakMap();this.screen=new THREE.Matrix4();this.frustum=new THREE.Frustum();this.size=new THREE.Vector2();
  core.addView('interfaceInput',Float64Array,()=>core.rust.interface_input_ptr(),()=>core.rust.interface_input_len());core.addView('uiDirty',Float64Array,()=>core.rust.ui_dirty_ptr(),()=>core.rust.ui_dirty_len());
 }
 render(scene,camera){const core=this.core,renderer=core.renderer;if(core.contextLost)return;if(core.failure)throw core.failure;
  if(scene.matrixWorldAutoUpdate)scene.updateMatrixWorld();if(camera.parent===null&&camera.matrixWorldAutoUpdate)camera.updateMatrixWorld();scene.onBeforeRender(renderer,scene,camera,renderer.getRenderTarget());
  this.screen.multiplyMatrices(camera.projectionMatrix,camera.matrixWorldInverse);this.frustum.setFromProjectionMatrix(this.screen);renderer.getSize(this.size);this.ratio=renderer.getPixelRatio();core.frame.interfaceBegin();this.frame++;this.count=0;this.walk(scene,camera,0);
  core.stale=true;core.settle();core.views();core.rust.ui_resources_frame(core.resources.gpu);core.views();
  for(let i=0;i<this.count;i++)this.write(this.rows[i],i,camera);
  if(core.frame.active)core.frame.interfaceEnd(this.count);else core.rust.interface_frame(this.count);core.stale=true;scene.onAfterRender(renderer,scene,camera);
 }
 // Hidden parents suppress descendants; layer mismatch affects only that node.
 // Material/group identity is resolved cold, without transient group objects.
 walk(node,camera,order){if(!node.visible)return;if(node.layers.test(camera.layers)){if(node.isGroup)order=node.renderOrder;
  if(node.userData.actorAsset&&(!node.frustumCulled||this.frustum.intersectsObject(node))){
   if(node.isSkinnedMesh)this.skin(node);
   const batch=node.userData.interfaceBatch,looks=batch?batch.looks:node.material,groups=batch?batch.drawGroups:null;
   if(batch){for(let i=0;i<groups.length;i++){const group=groups[i];this.add(node,looks[group.materialIndex],group.start,group.count,camera,order);}}
   else if(Array.isArray(looks)){const source=this.source(node);for(let i=0;i<source.facts.groups.length;i++){const g=source.facts.groups[i];this.add(node,looks[g[2]],g[0],g[1],camera,order);}}
   else{const source=this.source(node);this.add(node,looks,0,source.facts.indexCount||source.facts.count,camera,order);}
  }
 }for(let i=0;i<node.children.length;i++)this.walk(node.children[i],camera,order);}
 source(node){let entry=this.nodes.get(node);if(!entry||entry.asset!==node.userData.actorAsset){entry={asset:node.userData.actorAsset,source:nativeActorSource(this.core,node.userData.actorAsset),looks:new WeakMap(),streams:new Map()};this.nodes.set(node,entry);}return entry.source;}
 skin(node){const skeleton=node.skeleton;if(this.skeletons.get(skeleton)===this.frame)return;skeleton.update();publishActorSkin(this.core,skeleton,-this.frame-1);this.skeletons.set(skeleton,this.frame);}
 // One immutable descriptor per source/appearance/skin. The renderer may grow
 // while registering it, so acquire live views only after admission completes.
 admit(node,look){if(!look?.interfaceAppearance)return null;const source=this.source(node),entry=this.nodes.get(node);let record=entry.looks.get(look);const skin=node.skeleton?actorSkin(this.core,node.skeleton):null;if(entry.looks.has(look)&&(!record||record.skin===skin))return record;
  if(node.skeleton&&!skin){entry.looks.set(look,null);return null;}
  const core=this.core,map=imageSample(core.resources,look.map),mask=imageSample(core.resources,look.mask??look.map);
  const maps=Int32Array.of(map<0?-1:core.texture(map),mask<0?-1:core.texture(mask),skin?skin.texture:-1,+look.masked),samples=Int32Array.of(map,mask,skin?skin.sample:-1,-1);
  const facts=core.rust.ui_appearance(source.id,look.authored,Boolean(node.skeleton),maps,samples,look.id);if(!facts.length){entry.looks.set(look,null);return null;}
  const geometry=facts[U.META.GEOMETRY];let state=entry.streams.get(geometry);if(!state){state={bindings:[],serial:-1};for(let i=0;i<facts[U.META.BINDINGS];i++){const id=facts[U.META.FIRST+i],view='uiStream'+id;core.addView(view,Float32Array,()=>core.rust.ui_stream_ptr(id),()=>core.rust.ui_stream_len(id));state.bindings.push({id,view});}entry.streams.set(geometry,state);}
  record={handle:facts[U.META.APPEARANCE],look,skin,skeleton:node.skeleton,state};
  entry.looks.set(look,record);core.buffer=null;core.views();return record;
 }
 add(node,look,first,count,camera,order){if(!look?.interfaceAppearance||count<=0)return;if(this.count>=UI.MAX_DRAWS){this.core.rust.frame_overflow_frame();return;}const descriptor=this.admit(node,look);if(!descriptor)return;
  this.streams(node,descriptor);let row=this.rows[this.count];if(!row)this.rows[this.count]=row={};row.node=node;row.descriptor=descriptor;row.first=first;row.count=count;row.order=order;
  const b=node.boundingSphere?.center;const e=node.matrixWorld.elements,s=this.screen.elements,x=b?.x||0,y=b?.y||0,z=b?.z||0;const wx=e[0]*x+e[4]*y+e[8]*z+e[12],wy=e[1]*x+e[5]*y+e[9]*z+e[13],wz=e[2]*x+e[6]*y+e[10]*z+e[14];row.depth=s[2]*wx+s[6]*wy+s[10]*wz+s[14];this.count++;
 }
 // A source's arrays are publication data, not BufferAttributes. Dirty spans
 // feed the existing native frame-upload queue in one coarse resource sweep.
 streams(node,record){const batch=node.userData.interfaceBatch,points=node.userData.interfacePoints;if(!batch&&!points)return;const serial=batch?batch.serial:points.serial,state=record.state;if(state.serial===serial)return;
  const full=state.serial<0;this.core.views();for(let k=0;k<state.bindings.length;k++){const binding=state.bindings[k],out=this.core[binding.view],size=k===0?12:k===3?16:8;const data=batch?(k===0?batch.position:k===1?batch.uv:k===2?batch.maskUv:batch.tint):points.positions;
   const first=batch?(full?0:batch.first)*size:0,last=batch?(full?batch.count:batch.last)*size:data.length;for(let i=first;i<last;i++)out[i]=data[i];const at=binding.id*U.LIMIT.STREAM_WORDS;this.core.uiDirty[at+U.STREAM.DIRTY]=1;this.core.uiDirty[at+U.STREAM.FIRST]=first;this.core.uiDirty[at+U.STREAM.LAST]=last;
  }state.serial=serial;
 }
 write(row,index,camera){const out=this.core.interfaceInput,at=index*UI.STRIDE,node=row.node,record=row.descriptor,raw=at+UI.VALUES;
  out[at+UI.PROGRAM]=U.KIND.INTERFACE;out[at+UI.GEOMETRY]=record.handle;out[at+UI.FIRST]=row.first;out[at+UI.COUNT]=row.count;out[at+UI.GROUP_ORDER]=row.order;out[at+UI.RENDER_ORDER]=node.renderOrder;out[at+UI.DEPTH]=row.depth;out[at+UI.OBJECT]=node.id;
  out.set(node.matrixWorld.elements,raw+U.RAW.WORLD);out.set(camera.matrixWorldInverse.elements,raw+U.RAW.VIEW);out.set(camera.projectionMatrix.elements,raw+U.RAW.PROJECTION);if(node.skeleton){out.set(node.bindMatrix.elements,raw+U.RAW.BIND);out.set(node.bindMatrixInverse.elements,raw+U.RAW.INVERSE);}out.set(record.look.values,raw+U.RAW.LOOK);out[raw+U.RAW.PIXEL_RATIO]=this.ratio;out[raw+U.RAW.HEIGHT]=this.size.y;
 }
}
