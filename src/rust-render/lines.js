// LinesPass keeps the utility compiler until its program catalog replaces it.
// Native descriptors own cable/helper geometry, colour conversion and raster
// policy. A view publishes all visible placements and dirty point prefixes once.
import * as THREE from 'three';
import { LINE as D } from './line-layout.js';
import { compileFor } from './instances.js';
import { plainProgram } from './post.js';

class LinesPass {
  constructor(core){
    this.core=core;this.name='lines';this.phase='lines';this.records=new Map();this.sources=[];this.discoverAdded=object=>this.discover(object,this.camera,this.scene);this.visible=new Array(D.LIMIT.SOURCES);this.count=0;
    core.addView('lineInput',Float64Array,()=>core.rust.line_input_ptr(),()=>core.rust.line_input_len());
    core.addView('lineData',Float32Array,()=>core.rust.line_data_ptr(),()=>core.rust.line_data_len());
  }
  owns(object){return this.records.has(object);}
  // Program compilation uses disposable utility inputs. No LineBasicMaterial
  // or BufferGeometry survives admission as the source's CPU resource shell.
  discover(object,camera,scene){
    if(this.records.has(object))return;
    const source=object.userData.lineSource;if(!source)return;
    const geometry=new THREE.BufferGeometry();geometry.setAttribute('position',new THREE.BufferAttribute(new Float32Array(3),3));
    if(source.kind===D.KIND.AXES)geometry.setAttribute('color',new THREE.BufferAttribute(new Float32Array(3),3));
    const material=new THREE.LineBasicMaterial({vertexColors:source.kind===D.KIND.AXES});
    const shell=source.kind===D.KIND.TOW?new THREE.Line(geometry,material):new THREE.LineSegments(geometry,material);
    let program,id;
    try{
      compileFor(this.core.renderer,[[shell,material]],camera,scene);
      program=plainProgram(this.core,material,'position:0,color:1');
      if(!program)throw Error('Missing utility line program');
      id=this.core.rust.line_source(program.id,source.kind,source.capacity);
    }finally{material.dispose();geometry.dispose();}
    this.core.buffer=null;this.core.views();
    const record={object,source,id,serial:NaN};this.records.set(object,record);this.sources.push(record);
  }
  beforeSort(renderer,scene,camera,target,added){
    const core=this.core,g=core.graphics;
    if(scene!==g.scene||(camera!==g.camera||target!==g.sceneTarget)&&camera.userData?.mirror!==true)return;
    this.camera=camera;this.scene=scene;
    if(added)for(const root of added)root.traverse(this.discoverAdded);
    this.count=0;
    for(let i=0;i<this.sources.length;i++){
      const record=this.sources[i],object=record.object;
      if(record.id===undefined||!object.visible||!object.layers.test(camera.layers))continue;
      let root=object;while(root.parent&&root.visible)root=root.parent;
      if(!root.visible||root!==scene)continue;
      object.updateWorldMatrix(true,false);
      if(this.count<D.LIMIT.SOURCES)this.visible[this.count++]=object;
    }
  }
  // Native frame packets preserve each mirrored/main view's matrices. Live
  // points are the same world geometry in those views and upload by serial.
  prepare(ctx,view){
    const core=this.core;core.views();const input=core.lineInput,data=core.lineData;
    input.set(ctx.camera.projectionMatrix.elements,D.VIEW.PROJECTION);
    input.set(ctx.camera.matrixWorldInverse.elements,D.VIEW.INVERSE);
    input.set(ctx.camera.matrixWorld.elements,D.VIEW.WORLD);input[D.VIEW.ORTHOGRAPHIC]=+!!ctx.camera.isOrthographicCamera;
    let n=0;
    for(let i=0;i<this.count;i++){
      const object=this.visible[i];
      if(n===D.LIMIT.SOURCES)break;
      const record=this.records.get(object),source=record.source,at=D.VIEW.LEN+n*D.ROW.LEN;
      const count=Math.max(0,Math.min(source.count,source.capacity,D.LIMIT.VERTICES));
      input[at+D.ROW.SOURCE]=record.id;input[at+D.ROW.COUNT]=count;input[at+D.ROW.SERIAL]=source.serial;
      input.set(object.matrixWorld.elements,at+D.ROW.WORLD);
      let group=0;for(let parent=object.parent;parent;parent=parent.parent)if(parent.isGroup){group=parent.renderOrder;break;}
      input[at+D.ROW.GROUP_ORDER]=group;input[at+D.ROW.RENDER_ORDER]=object.renderOrder;input[at+D.ROW.OBJECT]=object.id;
      if(record.serial!==source.serial){
        const first=record.id*D.LIMIT.VERTICES*3;for(let i=0;i<count*3;i++)data[first+i]=source.positions[i];
        record.serial=source.serial;
      }
      n++;
    }
    input[D.VIEW.COUNT]=n;core.rust.line_frame(view);core.stale=true;
  }
}
export function linesPass(core){return new LinesPass(core);}
