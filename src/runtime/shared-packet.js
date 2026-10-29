// Per-player identity and presentation metadata share one authoritative binary
// world delta. Only the small JSON tail differs between recipients.
const encoder = new TextEncoder(), decoder = new TextDecoder();
export function packetParts(buffer) {
  const header = new Uint32Array(buffer, 0, 8);
  let offset = 32 + header[1]; offset = (offset + 1) & ~1;
  offset += header[2] * 2; offset = (offset + 3) & ~3;
  offset += header[3] * 4; offset = (offset + 7) & ~7;
  offset += header[4] * 8;
  return { buffer, offset, meta: JSON.parse(decoder.decode(new Uint8Array(buffer, offset, header[5]))) };
}
export function personalizePacket(parts, metadata) {
  metadata=expandMetadata(metadata);
  if(Array.isArray(metadata?.seats) && Array.isArray(parts.meta.seats)) {
    // Independent snapshots may reconstruct lease changes omitted by the
    // current source tick. Personal metadata must not erase those changes.
    const seats=new Map(parts.meta.seats);
    for(const [key,value]of metadata.seats)seats.set(key,value);
    if(Number.isSafeInteger(metadata.playerId))for(const [key,value]of seats) {
      if((key[0]==='v'||key[0]==='t') && Array.isArray(value))seats.set(key,[value[0],value[0]===metadata.playerId?1:0,...value.slice(2)]);
    }
    metadata={...metadata,seats:[...seats]};
  }
  const text = encoder.encode(JSON.stringify({ ...parts.meta, ...metadata }));
  const result = new ArrayBuffer(parts.offset + text.length);
  new Uint8Array(result, 0, parts.offset).set(new Uint8Array(parts.buffer, 0, parts.offset));
  new Uint32Array(result, 0, 8)[5] = text.length;
  new Uint8Array(result, parts.offset).set(text);
  return result;
}

// Negotiated transport optimization: fixed personal metadata schema avoids
// repeating property names 20 times a second for every connected player.
// Positions/timers were already quantized by the world; integers preserve
// those exact fractions with much shorter decimal JSON representations.
const scaled=(value,scale)=>Number.isFinite(value)?Math.round(value*scale):null;
const vector=value=>Array.isArray(value)?value.map(n=>scaled(n,1024)):null;
const unvector=value=>Array.isArray(value)?value.map(n=>n/1024):undefined;
const timer=value=>scaled(value,65536);
const untimer=value=>value==null?undefined:value/65536;
const SPAWN_KEYS=['phase','className','post','spawnedOnce','deadTime','pending','ended','auto'];
const HEALTH_KEYS=['stepMs','packetMs','bytes','tickMs','debt','dropped','late','queued'];
export function compactMetadata(meta) {
  const p=meta.personal || {},c=p.combat || {},s=p.soldier || {};
  const personal=[SPAWN_KEYS.map(key=>p.spawn?.[key] ?? null),timer(p.deadTime),timer(c.deathTime),
    vector(c.origin),vector(c.pivot),vector(c.aimDir),vector(s.settled),timer(s.stateTime),
    (p.weapons||[]).map(w=>[timer(w.wait),timer(w.kick)])];
  // Keep extensions intact; additions to the protocol cannot silently disappear.
  const {ack,playerId,generation,alive,playerClass,playerTeam,seats,personal:ignored,health,...extra}=meta;
  return {...extra,compact:1,values:[ack,playerId,generation,alive,playerClass,playerTeam,seats,personal,HEALTH_KEYS.map(key=>health?.[key]??0)]};
}
export function expandMetadata(meta) {
  if(meta?.compact!==1)return meta;
  const {compact,values,...extra}=meta;
  if(!Array.isArray(values)||values.length!==9)throw new Error('invalid compact player metadata');
  const [ack,playerId,generation,alive,playerClass,playerTeam,seats,p,health]=values;
  if(!Array.isArray(p)||p.length!==9||!Array.isArray(p[0])||!Array.isArray(p[8])||!Array.isArray(health))throw new Error('invalid compact player state');
  return {...extra,ack,playerId,generation,alive,playerClass,playerTeam,seats,
    personal:{spawn:Object.fromEntries(SPAWN_KEYS.map((key,i)=>[key,p[0][i]])),deadTime:untimer(p[1]),
      combat:{deathTime:untimer(p[2]),origin:unvector(p[3]),pivot:unvector(p[4]),aimDir:unvector(p[5])},
      soldier:{settled:unvector(p[6]),stateTime:untimer(p[7])},weapons:p[8].map(w=>({wait:untimer(w[0]),kick:untimer(w[1])}))},
    health:Object.fromEntries(HEALTH_KEYS.map((key,i)=>[key,health[i]]))};
}
