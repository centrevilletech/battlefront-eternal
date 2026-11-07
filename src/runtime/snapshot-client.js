// Rust retains baselines, display samples and changed gameplay facts. The
// transport lends a packet until receive is called again; resources retain
// native row leases when a prerequisite or history must outlive that borrow.
import {DEFAULT_SNAPSHOT_BOUNDS} from './snapshot-codec.js';
import {createPageNetwork} from './page-network.js';
import {RecordViews,RecordList} from './network-records.js';
import {NetHeader as H,NetKind as K} from './network-layout.js';
export class SnapshotClient {
  constructor({bounds=DEFAULT_SNAPSHOT_BOUNDS,onMissingBaseline=()=>{},onRecordsFull=()=>{},kernel=createPageNetwork()}={}){
    this.bounds=bounds;this.onMissingBaseline=onMissingBaseline;this.onRecordsFull=onRecordsFull;this.network=kernel;kernel.receiveSetup(bounds);
    this.epoch=NaN;this.ackTick=NaN;this.missingBaselines=NaN;this.refused=0;
    this.ack={epoch:NaN,tick:NaN};this.views=new RecordViews(kernel);
    this.records=new RecordList(this.views,false);this.seatChanges=new RecordList(this.views,true);
    this.packet={tick:NaN,epoch:NaN,epochChanged:false,interval:NaN,ackInput:NaN,ack:this.ack,records:this.records,seatChanges:this.seatChanges,snapshotAck:this.ack,complete:false};
    this.missing={magic:NaN,epoch:NaN,tick:NaN,interval:NaN,baselineTick:NaN,partIndex:NaN,partCount:NaN,ackInput:NaN,headerBytes:NaN};this.reset(0);
  }
  reset(epoch){this.network.reset(epoch);this.epoch=epoch;this.ackTick=0;this.missingBaselines=0;this.ack.epoch=epoch;this.ack.tick=0;}
  get snapshotAck(){return this.ack;}
  // Packet metadata and record lists are retained transport views. Lookups
  // allocate separate native leases, leaving this packet's publication intact.
  receive(bytes){
    const k=this.network;if(k.disposed)return null;const valid=k.receive(bytes),h=k.header;
    if(this.epoch!==h[H.EPOCH])this.missingBaselines=0;
    this.epoch=h[H.EPOCH];this.ackTick=h[H.ACK_TICK];this.ack.epoch=this.epoch;this.ack.tick=this.ackTick;
    if(h[H.REFUSED]>this.refused){this.refused=h[H.REFUSED];this.onRecordsFull();}
    if(h[H.MISSING_BASELINES]>this.missingBaselines){
      this.missingBaselines=h[H.MISSING_BASELINES];const m=this.missing;
      m.magic=h[H.MAGIC];m.epoch=this.epoch;m.tick=h[H.TICK];m.interval=h[H.INTERVAL];m.baselineTick=h[H.BASELINE_TICK];m.partIndex=h[H.PART_INDEX];m.partCount=h[H.PART_COUNT];m.ackInput=h[H.ACK_INPUT];m.headerBytes=h[H.HEADER_BYTES];this.onMissingBaseline(m);
    }
    if(!valid)return null;const p=this.packet;
    p.tick=h[H.TICK];p.epoch=this.epoch;p.epochChanged=!!h[H.EPOCH_CHANGED];p.interval=h[H.INTERVAL];p.ackInput=h[H.ACK_INPUT];p.complete=!!h[H.COMPLETE];return p;
  }
  // Reliable transport already owns semantic payloads. Its event callbacks
  // receive those records directly, independently from snapshot row leases.
  gameplayRecords(records,tick){return {tick,epoch:this.epoch,epochChanged:false,interval:2,ackInput:0,ack:this.ack,records,seatChanges:[],snapshotAck:this.epoch>=1?this.ack:null,complete:false};}
  get(kind,id){const code=K[kind.toUpperCase()],k=this.network;if(code===undefined||!k.lookup(code,id))return undefined;return this.views.rows[k.lookupIndex];}
  dispose(){this.network.dispose();}
}
