// Explicit presentation state beside a body's motion record. Every weapon is
// covered (including the secondary thrower); renderers never run its gameplay.
const NUMBERS=['heat','wait','reloadLeft','throwTime','charge','held','lockElapsed'];
const DEFAULTS={throwTime:-1,charge:1};
const FLAGS=['overheated','released','hidden','charging','lockComplete'];
export const UNIT_STATE_SCHEMA=6, UNIT_STATE_ID=100000;
// decodeUnitState and applyUnitState spell these fields out, in this order.
if(NUMBERS.join()!=='heat,wait,reloadLeft,throwTime,charge,held,lockElapsed'||FLAGS.join()!=='overheated,released,hidden,charging,lockComplete'
  ||DEFAULTS.throwTime!==-1||DEFAULTS.charge!==1||Object.keys(DEFAULTS).length!==2)throw Error('unit state fields changed: update decodeUnitState and applyUnitState');
const encoder=new TextEncoder(),decoder=new TextDecoder('utf-8',{fatal:true});
export function captureUnitState(unit,registry,tick){
  const weapons=unit.combat.arsenal.weapons;
  if(weapons.length>12)throw Error('Unit weapon count exceeds presentation limit');
  const repair=encoder.encode(unit.combat.repairLock||'');if(repair.length>128)throw Error('Repair identity too long');
  const payload=new Uint8Array(21+repair.length+weapons.length*39),v=new DataView(payload.buffer);let at=0;
  const u8=n=>v.setUint8(at++,n),u16=n=>{v.setUint16(at,n,true);at+=2;},u32=n=>{v.setUint32(at,n,true);at+=4;},f=n=>{if(!Number.isFinite(n))throw Error('Invalid unit presentation number');v.setFloat32(at,n,true);at+=4;};
  u32(unit.id);u8((unit.hero?1:0)|(unit.targetable?2:0)|(unit.actor?.root.visible?4:0)|(unit.combat.blowApart?8:0));f(unit.deadTime||0);f(unit.soldier.stateTime||0);
  u16(unit.character?unit.character.rosterSlot??65535:65535);
  const arsenal=unit.combat.arsenal;
  for(let channel=0;channel<2;channel++)u16(registry.weaponIds.get(arsenal.current(channel)?.name)??0);
  u8(weapons.length);
  u8(repair.length);payload.set(repair,at);at+=repair.length;
  for(const w of weapons){
    const id=registry.weaponIds.get(w.name);if(!id)throw Error('Unknown unit weapon');
    u16(id);u8(w.levels.indexOf(w.def));u8(FLAGS.reduce((n,k,i)=>n|(w[k]?1<<i:0),0));
    // Idle weapon timers dominate these records. Omit exact weapon defaults,
    // preserving the same Float32 values for active presentation state.
    const target=w.lockTargetId??-1,generation=w.lockGeneration??-1;
    const mask=NUMBERS.reduce((bits,k,i)=>bits|((w[k]??DEFAULTS[k]??0)!==(DEFAULTS[k]??0)?1<<i:0),0)|((target!==-1||generation!==-1)?128:0);
    u8(mask);for(const [i,k]of NUMBERS.entries())if(mask&(1<<i))f(w[k]);
    if(mask&128){u32(target);u16(generation);}
  }
  return {kind:'summary',id:UNIT_STATE_ID+unit.id,generation:unit.generation,tick,schema:UNIT_STATE_SCHEMA,payload:payload.slice(0,at)};
}
// Read in place: every remote body's state arrives every snapshot, so no
// reader closures over a shared offset. Checks and errors as written.
const finite=n=>{if(!Number.isFinite(n))throw Error('Invalid unit presentation number');return n;};
export function decodeUnitState(record,registry){
  const {payload}=record;if(record.schema!==UNIT_STATE_SCHEMA||!(payload instanceof Uint8Array)||payload.length<21)throw Error('Invalid unit presentation');
  const v=new DataView(payload.buffer,payload.byteOffset,payload.byteLength);
  const id=v.getUint32(0,true),flags=v.getUint8(4),deadTime=finite(v.getFloat32(5,true)),stateTime=finite(v.getFloat32(9,true)),slot=v.getUint16(13,true);
  const first=v.getUint16(15,true),second=v.getUint16(17,true),count=v.getUint8(19),size=v.getUint8(20);let at=21;
  if(record.id!==UNIT_STATE_ID+id||flags>15||size>128||count>12||payload.length<21+size+5*count)throw Error('Invalid unit presentation layout');
  const repairLock=size?decoder.decode(payload.subarray(at,at+size)):'';at+=size;
  const weapons=[];
  for(let i=0;i<count;i++){
    const name=registry.weapons.get(v.getUint16(at,true)),level=v.getUint8(at+2),bits=v.getUint8(at+3);at+=4;if(!name||bits>31)throw Error('Invalid unit weapon');
    // NUMBERS in order, each present when its mask bit is (DEFAULTS else);
    // one literal per weapon, the fields in FLAGS then NUMBERS order.
    const mask=v.getUint8(at++);
    let heat=0,wait=0,reloadLeft=0,throwTime=-1,charge=1,held=0,lockElapsed=0;
    if(mask&1){heat=finite(v.getFloat32(at,true));at+=4;}
    if(mask&2){wait=finite(v.getFloat32(at,true));at+=4;}
    if(mask&4){reloadLeft=finite(v.getFloat32(at,true));at+=4;}
    if(mask&8){throwTime=finite(v.getFloat32(at,true));at+=4;}
    if(mask&16){charge=finite(v.getFloat32(at,true));at+=4;}
    if(mask&32){held=finite(v.getFloat32(at,true));at+=4;}
    if(mask&64){lockElapsed=finite(v.getFloat32(at,true));at+=4;}
    let lockTargetId=-1,lockGeneration=-1;
    if(mask&128){const target=v.getUint32(at,true),generation=v.getUint16(at+4,true);at+=6;lockTargetId=target===0xffffffff?-1:target;lockGeneration=generation===65535&&lockTargetId===-1?-1:generation;}
    weapons.push({name,level,overheated:!!(bits&1),released:!!(bits&2),hidden:!!(bits&4),charging:!!(bits&8),lockComplete:!!(bits&16),
      heat,wait,reloadLeft,throwTime,charge,held,lockElapsed,lockTargetId,lockGeneration});
  }
  if(at!==payload.length)throw Error('Invalid unit presentation layout');
  if((first&&!registry.weapons.has(first))||(second&&!registry.weapons.has(second)))throw Error('Unknown selected weapon');
  return {id,hero:!!(flags&1),targetable:!!(flags&2),visible:!!(flags&4),blowApart:!!(flags&8),repairLock,deadTime,stateTime,slot:slot===65535?null:slot,selected:[first?registry.weapons.get(first):null,second?registry.weapons.get(second):null],weapons};
}
export function applyUnitState(game,record,registry){
  const state=decodeUnitState(record,registry),unit=game.units.byId(state.id);
  if(!unit?.combat||unit.generation!==record.generation)return false;
  const arsenal=unit.combat.arsenal;
  const resolved=state.weapons.map(s=>{const w=arsenal.weapons.find(w=>w.name===s.name);if(!w||!w.levels[s.level])throw Error('Missing unit weapon prerequisite');return w;});
  unit.hero=state.hero;unit.targetable=state.targetable;
  unit.actor.root.visible=state.visible;unit.combat.blowApart=state.blowApart;unit.combat.repairLock=state.repairLock;
  // A dead droid blown apart is gone for this life (Unit.goUp), even if its
  // blow-apart event never arrived.
  if(unit.dead&&state.blowApart)unit.goUp?.();
  if(unit!==game.playerUnit){unit.deadTime=state.deadTime;unit.soldier.stateTime=state.stateTime;}
  if(state.slot!==null&&game.battle?.characters[state.slot]){unit.character=game.battle.characters[state.slot];if(unit.alive)unit.character.unit=unit;}
  // Exact own corrections cover every weapon now. Remote presentation must
  // never overwrite the private weapon state used for local replay and HUD.
  if(unit!==game.playerUnit)for(let i=0;i<resolved.length;i++){
    const w=resolved[i],s=state.weapons[i];w.def=w.levels[s.level];
    w.heat=s.heat;w.wait=s.wait;w.reloadLeft=s.reloadLeft;w.throwTime=s.throwTime;w.charge=s.charge;w.held=s.held;w.lockElapsed=s.lockElapsed;
    w.overheated=s.overheated;w.released=s.released;w.hidden=s.hidden;w.charging=s.charging;w.lockComplete=s.lockComplete;
    w.lockTargetId=s.lockTargetId;w.lockGeneration=s.lockGeneration;
  }
  if(unit!==game.playerUnit)for(const name of state.selected){const w=arsenal.weapons.find(w=>w.name===name);if(w)arsenal.select(w);}
  unit.combat.updateHeld?.();return true;
}
