// One generated dirty block crosses the presentation/render memories per phase.
// Cached chunks and a bounded tail avoid transient typed-array slices.
import {PoseHeader as H,PoseLimits as L} from '../presentation/scene-pose-layout.js';
import {ScenePoseRef as P} from './scene-layout.js';
export class ScenePoseOwner {
  constructor(registry,source){
    this.registry=registry;this.source=source;this.id=registry.poseOwners.length+1;
    this.generation=1;this.buffer=null;this.kernel=source.kernel;this.chunks=[];
    this.kernel.scene_pose_owner(this.id,this.generation);
  }
  // Cold model admission declares its world consumers and skin span together.
  // Aggregate skins consume the retained bone worlds without Three math.
  model(row,objects,start,id,skinStart,skinCount){
    const registry=this.registry,core=registry.core;row.scenePoseOwner=this;
    if(this.id>=L.OWNERS){registry.clipped++;return;}
    const targets=new Uint8Array(objects.length);
    for(let i=0;i<objects.length;i++){
      const object=objects[i];
      targets[i]=+(object!==null);
    }
    this.kernel.scene_pose_targets(start,targets,id+1);this.buffer=null;
    if(!core.rust.scene_pose_register(this.id,this.generation,id,this.generation,start,objects.length,skinStart,skinCount)){registry.clipped++;return;}
    core.buffer=null;core.views();
    for(let i=0;i<objects.length;i++){
      const object=objects[i];if(!object)continue;
      const node=registry.admitIfNew(object);if(!node)continue;
      node.transformNative=true;node.transformStop=true;
      if(!targets[i])continue;
      const pose=node.pose ||= new Float64Array(P.STRIDE);
      pose[P.OWNER]=this.id;pose[P.ID]=id;pose[P.GENERATION]=this.generation;
      pose[P.NODE_START]=start+i;pose[P.NODE_COUNT]=1;pose[P.SKIN_START]=skinStart;pose[P.SKIN_COUNT]=skinCount;
      registry.touch(node);registry.transforms.touch(node);
    }
    row.scenePoseOwner=this;this.kernel.scene_pose_admit_frame();this.publish();
  }
  visible(scene,places,rows){
    const registry=this.registry,core=registry.core,root=registry.admitIfNew(scene);registry.worlds.flush();
    if(!registry.placeView){core.addView('scenePlaces',Uint32Array,()=>core.rust.scene_place_output_ptr(),()=>core.rust.scene_place_output_len());registry.placeView=true;}
    const count=core.rust.scene_place_frame(root.slot,root.generation,this.id,rows.length);core.views();let n=0;
    for(let i=0;i<count;i++){const id=core.scenePlaces[i];if(rows[id]?.stale)places[n++]=id;}
    return n;
  }
  refresh(){
    const source=this.source,buffer=source.loaded.memory.buffer;
    if(buffer===this.buffer)return;
    this.buffer=buffer;const offset=this.kernel.scene_pose_ptr(),length=this.kernel.scene_pose_len();
    this.input=new Float64Array(buffer,offset,length);this.words=new Uint32Array(buffer,offset,length*2);
    const chunks=this.chunks;chunks.length=Math.floor(this.words.length/L.COPY_WORDS);
    for(let i=0;i<chunks.length;i++)chunks[i]=new Uint32Array(buffer,offset+i*L.COPY_WORDS*4,L.COPY_WORDS);
  }
  // A sequence also retires zero-delta phases. A replayed block cannot advance
  // a model or its motion history twice, even after a long paused interval.
  publish(){
    this.refresh();const core=this.registry.core;core.views();
    const words=Math.min(Math.max(H.STRIDE,this.input[H.WORDS])*2,this.words.length,core.scenePoseWords.length);
    let at=0,chunk=0;
    for(;at+L.COPY_WORDS<=words;at+=L.COPY_WORDS)core.scenePoseWords.set(this.chunks[chunk++],at);
    for(;at<words;at++)core.scenePoseWords[at]=this.words[at];
    if(core.rust.scene_pose_frame())this.registry.lastValid=false;
  }
}
