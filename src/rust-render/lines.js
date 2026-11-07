// Native line descriptors use the owned finite utility program catalog.
// Native descriptors own cable/helper geometry, colour conversion and raster
// policy. A view publishes all visible placements and dirty point prefixes once.
import {Family,Mode,S} from './scene.js';
import {SceneLineHeader as H} from './scene-layout.js';
import {ownedGeometryProgram} from './programs.js';
import {programLayouts} from '../render-programs.js';
const LINE_KEYS=Object.keys(programLayouts).filter(key=>key.startsWith('utility/line-'));
import { LINE as D } from './line-layout.js';


class LinesPass {
  constructor(core){
    this.core=core;this.name='lines';this.phase='lines';this.records=new Map();this.count=0;
    core.addView('lineInput',Float64Array,()=>core.rust.line_input_ptr(),()=>core.rust.line_input_len());
    core.addView('lineData',Float32Array,()=>core.rust.line_data_ptr(),()=>core.rust.line_data_len());
  }
  owns(object){return this.records.has(object);}
  // discover binds the native recipe to its owned utility key once. A later
  // view reuses that identity without compiling or reflecting a browser draw.
  discover(object,camera,scene){
    if(this.records.has(object))return;
    const source=object.userData.lineSource;if(!source)return;
    const program=ownedGeometryProgram(this.core,source.kind===D.KIND.AXES?'utility/line-3-solid':'utility/line-plain-solid');
    const id=this.core.rust.line_source(program.id,source.kind,source.capacity);
    this.core.buffer=null;this.core.views();
    const record={object,source,id,serial:NaN};this.records.set(object,record);this.core.registry.bind(object,Family.LINE,record,id);
  }
  beforeSort(renderer,scene,camera,target,added){
    const core=this.core,g=core.graphics;
    if(scene!==g.scene||(camera!==g.camera||target!==g.sceneTarget)&&camera.userData?.mirror!==true)return;
    for(const object of added||[])if(object.userData.lineSource)this.discover(object,camera,scene);
    this.count=core.registry.select(scene,camera,Family.LINE,Mode.VIEW,camera.layers.mask,D.LIMIT.SOURCES);
  }
  programs(){for(const key of LINE_KEYS)ownedGeometryProgram(this.core,key);}
  async warm(settle){for(const key of LINE_KEYS){ownedGeometryProgram(this.core,key);await settle();}}

  // Native frame packets preserve each mirrored/main view's matrices. Live
  // points are the same world geometry in those views and upload by serial.
  prepare(ctx,view){
    const core=this.core;core.views();const input=core.lineInput,data=core.lineData;
    input.set(ctx.camera.projectionMatrix.elements,D.VIEW.PROJECTION);
    input.set(ctx.camera.matrixWorldInverse.elements,D.VIEW.INVERSE);
    input.set(ctx.camera.matrixWorld.elements,D.VIEW.WORLD);input[D.VIEW.ORTHOGRAPHIC]=+!!ctx.camera.isOrthographicCamera;
    let n=0;
    for(let i=0;i<this.count;i++){
      const selected=H.STRIDE+i*S.STRIDE,object=core.registry.nodes[core.sceneLineList[selected+S.SLOT]].object;
      if(n===D.LIMIT.SOURCES)break;
      const record=this.records.get(object),source=record.source,at=D.VIEW.LEN+n*D.ROW.LEN;
      const count=Math.max(0,Math.min(source.count,source.capacity,D.LIMIT.VERTICES));
      input[at+D.ROW.SOURCE]=record.id;input[at+D.ROW.COUNT]=count;input[at+D.ROW.SERIAL]=source.serial;
      input.set(object.matrixWorld.elements,at+D.ROW.WORLD);
      input[at+D.ROW.GROUP_ORDER]=core.sceneLineList[selected+S.GROUP_ORDER];input[at+D.ROW.RENDER_ORDER]=core.sceneLineList[selected+S.RENDER_ORDER];input[at+D.ROW.OBJECT]=core.sceneLineList[selected+S.OBJECT_ID];
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
