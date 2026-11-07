// SnapshotHost's one publication operation owns its WASM handle and byte view.
// Transport copies spans out; no pointer or linear-memory view escapes it.
import { WirePublicationOutputMirror, syncWirePublicationOutput } from '../gen/index.js';
const OUTPUT={Mirror:WirePublicationOutputMirror,sync:syncWirePublicationOutput};
export class PublicationOperation {
  constructor(bridge,{runtime,source,stagger,shots,host,combat}) {
    this.bridge=bridge;this.source=source;this.combat=combat;this.x=bridge.x;
    this.handle=this.x.wire_publish_create(runtime.nativeHandle,source.nativeHandle,stagger.nativeHandle,shots.nativeHandle,host.nativeHandle,combat.native.handle);
    if(!this.handle)throw Error('publication attachment failed');
    this.output=new WirePublicationOutputMirror();
    this.bytes=null;this.base=0;
    this.facts={at:0,unixMs:0,droppedTicks:0};
  }
  bind(state,actor) {
    if(!this.x.wire_publish_bind(this.handle,state.nativeId,this.source.peerId(actor),state.edges.handle))throw Error('publication client attachment failed');
  }
  publish(tick,at,unixMs,droppedTicks) {
    const facts=this.facts;facts.at=at;facts.unixMs=unixMs;facts.droppedTicks=droppedTicks;
    this.source.writeWorldFacts(tick,facts);
    const b=this.bridge,block=b.readBlock(this.x.wire_publish(this.handle));
    const result=b.decode(OUTPUT,block,this.output);
    b.backend.blocks.refresh();this.bytes=b.backend.blocks.bytes;this.base=result.bytes;
    return result;
  }
  copy(span) {return this.bytes.slice(this.base+span.offset,this.base+span.offset+span.length);}
  status(span) {
    let text='';const bytes=this.bytes;
    for(let at=this.base+span.offset,end=at+span.length;at<end;at+=2)text+=String.fromCharCode(bytes[at]|bytes[at+1]<<8);
    return text;
  }
  close() {if(this.handle){this.x.wire_publish_drop(this.handle);this.handle=0;this.bytes=null;return true;}return false;}
}
