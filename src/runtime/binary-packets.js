const MAGIC=0x31424642;
export class BinaryPackets {
  constructor(deliver,{magic=MAGIC,lossy=false}={}){this.deliver=deliver;this.magic=magic;this.lossy=lossy;this.pending=new Map();}
  clear(){this.pending.clear();}
  accept(buffer){
    if(!(buffer instanceof ArrayBuffer)||buffer.byteLength<16||buffer.byteLength>32768)throw Error('Invalid binary packet');
    const view=new DataView(buffer),id=view.getUint32(4,true),index=view.getUint16(8,true),total=view.getUint16(10,true),length=view.getUint32(12,true);
    if(view.getUint32(0,true)!==this.magic||total<1||total>128||index>=total||length>8192||length>buffer.byteLength-16||index!==0&&length)throw Error('Invalid binary packet header');
    let metadata;
    if(length){metadata=JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(new Uint8Array(buffer,16,length)));if(!metadata||typeof metadata!=='object'||Array.isArray(metadata))throw Error('Invalid binary metadata');}
    for(const [key,value] of this.pending)if(Date.now()-value.at>(this.lossy?1000:10000))this.pending.delete(key);
    if(!this.pending.has(id)){if(this.pending.size>=8){if(this.lossy)this.pending.delete(this.pending.keys().next().value);else throw Error('Binary packet backlog exceeded');}this.pending.set(id,{total,at:Date.now(),parts:new Map(),bytes:0});}
    const pending=this.pending.get(id);
    if(pending.total!==total||pending.parts.has(index)){if(this.lossy)return;throw Error('Invalid duplicate binary fragment');}
    const data=new Uint8Array(buffer,16+length);
    if(data.byteLength>20000)throw Error('Binary fragment too large');
    pending.bytes+=data.byteLength;if(pending.bytes>2097152)throw Error('Binary packet too large');
    if(metadata!==undefined)pending.metadata=metadata;
    pending.parts.set(index,data);
    if(pending.parts.size!==total)return;
    this.pending.delete(id);
    const result=new Uint8Array(pending.bytes);let offset=0;
    for(let i=0;i<total;i++){const part=pending.parts.get(i);result.set(part,offset);offset+=part.length;}
    return this.deliver(result.buffer,pending.metadata,id);
  }
}
