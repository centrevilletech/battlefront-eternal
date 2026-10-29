// An explosion is an authoritative occurrence, not a client damage instruction.
export function explosionRecord({id,generation,tick,definitionId,position,wet}) {
 for(const n of [id,generation,tick,definitionId,tick+18])if(!Number.isSafeInteger(n)||n<0||n>0xffffffff)throw Error('invalid explosion identity');
 if(!Array.isArray(position)||position.length!==3)throw Error('invalid explosion position');
 const payload=new Uint8Array(33),v=new DataView(payload.buffer);
 v.setUint32(0,tick+18,true);v.setUint32(4,definitionId,true);
 for(let i=0;i<3;i++){if(!Number.isFinite(position[i]))throw Error('invalid explosion position');v.setFloat64(8+i*8,position[i],true);}
 v.setUint8(32,wet?1:0);
 return{kind:'combat',id,generation,tick,schema:5,payload};
}
export function decodeExplosion(record) {
 if(record.kind!=='combat'||record.schema!==5||record.payload?.length!==33)throw Error('invalid explosion record');
 const v=new DataView(record.payload.buffer,record.payload.byteOffset,record.payload.byteLength),expiryTick=v.getUint32(0,true),position=[];
 for(let i=0;i<3;i++){const n=v.getFloat64(8+i*8,true);if(!Number.isFinite(n))throw Error('invalid explosion position');position.push(n);}
 if(expiryTick!==record.tick+18||v.getUint8(32)>1)throw Error('invalid explosion expiry or water flag');
 return{...record,expiryTick,definitionId:v.getUint32(4,true),position,wet:Boolean(v.getUint8(32))};
}
