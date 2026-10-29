// Transport retention only: gameplay supplies the event, lifetime and identity.
export class EntityEventStore {
  constructor({maxRecords=4096,maxBytes=1048576}={}) {Object.assign(this,{maxRecords,maxBytes});this.records=new Map();this.bytes=0;this.dropped=0;}
  delete(id) {const entry=this.records.get(id);if(!entry)return;this.bytes-=entry.prepared.byteLength;this.records.delete(id);}
  update(envelopes,tick,prepare,decode) {
    for(const[id,event]of this.records)if(event.expiresTick<tick)this.delete(id);
    const changes=[];
    for(const envelope of envelopes){
      if(envelope.expiresTick<tick)continue;
      const prepared=prepare(envelope.record),id=prepared.record.id,old=this.records.get(id),event={...envelope,prepared,view:decode(prepared.record)};
      const full=()=>this.records.size+(old?0:1)>this.maxRecords||this.bytes-(old?.prepared.byteLength??0)+prepared.byteLength>this.maxBytes;
      if(full())for(const[key,entry]of this.records)if(key!==id&&entry.retention==='cosmetic'){this.delete(key);if(!full())break;}
      if(full()){this.dropped++;continue;}
      this.bytes+=prepared.byteLength-(old?.prepared.byteLength??0);this.records.set(id,event);changes.push(event);
    }
    return changes;
  }
}
