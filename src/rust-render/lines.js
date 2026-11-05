// Tow cables and flight/navigation helpers keep their authored line topology.
// Three compiles their basic/vertex-colour programs; Rust owns every draw and
// the retained dynamic buffers. Opaque lines precede the far image; transparent
// lines split the transparent command stream at their normal sort keys.
import * as THREE from "three";
import { compileFor } from "./instances.js";
import { plainProgram } from "./post.js";
import { writeScreenStep } from "./screen.js";
import { GL_TYPES } from "./geometry-types.js";

const MAX_STEPS = 8; // post.rs MAX_STEPS; each batch uses the existing step ABI.
const opaqueOrder=(a,b)=>a.key[0]-b.key[0]||a.key[1]-b.key[1]||a.material.id-b.material.id||a.key[2]-b.key[2]||a.key[3]-b.key[3];
class LinesPass {
  constructor(core){
    this.core=core;this.name="lines";this.phase="lines";
    this.records=new Map();this.pending=new Set();this.opaque=[];this.transparent=[];
    this.frustum=new THREE.Frustum();this.screen=new THREE.Matrix4();this.point=new THREE.Vector4();
    this.batch={rows:null,first:0,count:0};this.single=[];this.stats=null;
  }
  owns(object){return this.records.has(object)||this.pending.has(object);}
  beforeSort(renderer,scene,camera,target,added){
    const g=this.core.graphics;
    if(scene!==g.scene || (camera!==g.camera||target!==g.sceneTarget)&&camera.userData?.mirror!==true)return;
    for(const root of added||[])root.traverse(object=>{if(object.isLine)this.discover(object,camera,scene);});
    for(const object of this.pending)if(object.geometry.attributes.position){this.discover(object,camera,scene);this.pending.delete(object);}
    this.opaque.length=this.transparent.length=0;
    if(this.stats)this.stats.fill(0);
    this.screen.multiplyMatrices(camera.projectionMatrix,camera.matrixWorldInverse);this.frustum.setFromProjectionMatrix(this.screen);
    for(const [object,old] of this.records){
      const material=object.material;
      if(!object.visible||!material.visible||!object.layers.test(camera.layers))continue;
      let parent=object;while(parent.parent&&parent.visible)parent=parent.parent;
      if(!parent.visible||parent!==scene)continue;
      const record=old.material!==material||old.version!==material.version?this.discover(object,camera,scene):old;
      object.updateWorldMatrix(true,false);
      const geometry=object.geometry;
      if(!geometry.attributes.position||geometry.attributes.position.count===0)continue;
      if(object.frustumCulled){if(!geometry.boundingSphere)geometry.computeBoundingSphere();if(!this.frustum.intersectsObject(object))continue;}
      this.upload(record);
      const first=Math.max(0,geometry.drawRange.start),available=geometry.index?.count??geometry.attributes.position.count;
      record.count=Math.max(0,Math.min(available-first,geometry.drawRange.count));
      if(!record.count)continue;
      const indexed=Boolean(geometry.index),width=material.linewidth||1;
      if(record.first!==first||record.indexed!==indexed||record.width!==width){
        this.core.rust.geometry_primitive(record.geometry,object.isLineSegments?1:object.isLineLoop?2:3,first,indexed,width);
        record.first=first;record.indexed=indexed;record.width=width;
      }
      if(!geometry.boundingSphere)geometry.computeBoundingSphere();
      const c=geometry.boundingSphere.center;this.point.set(c.x,c.y,c.z,1).applyMatrix4(object.matrixWorld).applyMatrix4(this.screen);
      let group=object.parent;while(group&&!group.isGroup)group=group.parent;
      record.key[0]=group?.renderOrder??0;record.key[1]=object.renderOrder;record.key[2]=this.point.z;record.key[3]=object.id;
      if(material.transparent)this.transparent.push(record.command);else this.opaque.push(record);
    }
    this.opaque.sort(opaqueOrder);
  }
  discover(object,camera,scene){
    const core=this.core,renderer=core.renderer,material=object.material;
    if(!material?.isLineBasicMaterial||!material.colorWrite||material.stencilWrite||material.polygonOffset||material.alphaToCoverage)core.fail(Error(`Unsupported battle line material: ${material?.type}`));
    if(!object.geometry.attributes.position){this.pending.add(object);return null;}
    compileFor(renderer,[[object,material]],camera,scene);
    const native=renderer.properties.get(material).currentProgram;
    if(!native)core.fail(Error("Line program did not compile"));
    const program=plainProgram(core,material,null);
    if(!program)core.fail(Error("Cannot bind battle line program"));
    const record={object,material,version:material.version,program,geometry:core.rust.add_geometry(),buffers:[],key:new Float64Array(4),count:0,first:-1,indexed:null,width:0,index:null};
    record.command={line:record,key:record.key};this.records.set(object,record);
    this.upload(record);core.stale=true;return record;
  }
  upload(record){
    const core=this.core,geometry=record.object.geometry;
    for(let i=0;i<record.program.attributes.length;i++){
      const [name,location]=record.program.attributes[i],attribute=geometry.attributes[name],source=attribute?.isInterleavedBufferAttribute?attribute.data:attribute,array=source?.array;
      if(!array||GL_TYPES[array.constructor.name]===undefined)core.fail(Error(`Missing line attribute ${name}`));
      let buffer=record.buffers[i];
      if(!buffer){buffer={slot:core.rust.geometry_buffer(record.geometry,new Uint8Array(array.buffer,array.byteOffset,array.byteLength)),array,version:source.version,bytes:new Uint8Array(array.buffer,array.byteOffset,array.byteLength)};record.buffers[i]=buffer;}
      else if(buffer.array!==array||buffer.version!==source.version){
        if(buffer.array!==array){buffer.array=array;buffer.bytes=new Uint8Array(array.buffer,array.byteOffset,array.byteLength);}
        core.rust.geometry_buffer_data(record.geometry,buffer.slot,buffer.bytes);buffer.version=source.version;core.stale=true;
      }
      const stride=attribute.isInterleavedBufferAttribute?source.stride*array.BYTES_PER_ELEMENT:0,offset=attribute.isInterleavedBufferAttribute?attribute.offset*array.BYTES_PER_ELEMENT:0;
      const type=GL_TYPES[array.constructor.name];
      if(buffer.location!==location||buffer.size!==attribute.itemSize||buffer.type!==type||buffer.normalized!==attribute.normalized||buffer.stride!==stride||buffer.offset!==offset){
        core.rust.geometry_pointer(record.geometry,buffer.slot,location,attribute.itemSize,type,attribute.normalized,stride,offset);
        buffer.location=location;buffer.size=attribute.itemSize;buffer.type=type;buffer.normalized=attribute.normalized;buffer.stride=stride;buffer.offset=offset;
      }
    }
    const index=geometry.index;
    if(index){
      if(!record.index||record.index.type!==index.array.constructor){core.rust.geometry_index(record.geometry,new Uint8Array(index.array.buffer,index.array.byteOffset,index.array.byteLength),GL_TYPES[index.array.constructor.name]);record.index={array:index.array,type:index.array.constructor,version:index.version,bytes:new Uint8Array(index.array.buffer,index.array.byteOffset,index.array.byteLength)};}
      else if(record.index.array!==index.array||record.index.version!==index.version){if(record.index.array!==index.array)record.index.bytes=new Uint8Array(index.array.buffer,index.array.byteOffset,index.array.byteLength);core.rust.geometry_index_data(record.geometry,record.index.bytes);record.index.array=index.array;record.index.version=index.version;}
    }
  }
  draw(ctx){
    const {rows,first,count}=this.batch;
    for(let i=0;i<count;i++){const record=rows[first+i];
      record.object.onBeforeRender(ctx.renderer,ctx.scene,ctx.camera,record.object.geometry,record.material,null);
      if(!writeScreenStep(this.core,i,record.program,record.material,record.object,ctx.camera,{geometry:{id:record.geometry,count:record.count}}))this.core.fail(Error("Cannot write battle line uniforms"));
    }
    ctx.steps=count;
  }
  raster(rows,first,count,ctx){
    this.batch.rows=rows;this.batch.first=first;this.batch.count=count;
    this.core.phase("lines",ctx.renderer,ctx.scene,ctx.camera,true);
    const stats=this.core.phaseStats.lines;
    if(!this.stats)this.stats=new Uint32Array(stats.length);
    for(let i=0;i<stats.length;i++)this.stats[i]+=stats[i];
    this.core.phaseStats.lines=this.stats;
    for(let i=0;i<count;i++){const r=rows[first+i];r.object.onAfterRender(ctx.renderer,ctx.scene,ctx.camera,r.object.geometry,r.material,null);}
  }
  drawOpaque(ctx){for(let first=0;first<this.opaque.length;first+=MAX_STEPS)this.raster(this.opaque,first,Math.min(MAX_STEPS,this.opaque.length-first),ctx);}
  drawOne(record,ctx){this.single[0]=record;this.raster(this.single,0,1,ctx);}
}
export function linesPass(core){return new LinesPass(core);}
