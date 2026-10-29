// Persistent station, tow and creature state. Summary identities have their own
// ranges; these records repair missed changes and establish a late join without
// a snapshot. A creature that seizes (src/props.js AnimatedProp: the sarlacc's
// tentacles, the rancor) is the world's alone: its clip, when it started, and
// whom it holds since when, which the page follows and draws him from. A
// trap (src/traps.js, Endor's log traps) is the world's too: its phase (down,
// being hauled up, cocked, swinging) and the tick it began, which the page
// draws its clip from; its built state, health and team go as any building's.
export const BUILDING_STATE_SCHEMA=7, TOW_STATE_SCHEMA=8, CREATURE_STATE_SCHEMA=9, CREATURE_STATE_ID=400000, TRAP_STATE_SCHEMA=10, TRAP_STATE_ID=500000;
const encode=new TextEncoder(),decode=new TextDecoder('utf-8',{fatal:true});
function make(id,generation,tick,schema,value){const payload=encode.encode(JSON.stringify(value));if(payload.length>900)throw Error('World presentation record too large');return {kind:'summary',id,generation,tick,schema,payload};}
export function captureWorldPresentation(game,tick,epoch){
  const records=[];
  for(const [index,b]of (game.live?.buildings||[]).entries())if(!(game.turretSeats?.turrets||[]).includes(b))records.push(make(200000+index,epoch,tick,7,[index,b.state,Number.isFinite(b.health)?b.health:null,b.team||0,b.mode??null,b.active||0,b.pulse||0,b.facing||0,b.yaw||0,b.servedHealth||false,b.servedAmmo||false]));
  for(const [index,prop]of (game.live?.animated||[]).entries())if(prop.grabs?.length&&prop.slot>=0)records.push(make(CREATURE_STATE_ID+index,epoch,tick,CREATURE_STATE_SCHEMA,[index,...prop.state()]));
  for(const [index,trap]of (game.live?.traps||[]).entries())records.push(make(TRAP_STATE_ID+index,epoch,tick,TRAP_STATE_SCHEMA,[index,...trap.stateRecord()]));
  for(const v of game.vehicles?.vehicles||[])if(v.alive){const target=game.vehicles.vehicles[v.towTarget];records.push(make(300000+v.aiTarget.id,v.aiTarget.generation,tick,8,[v.aiTarget.id,target?.aiTarget.id||0,target?.aiTarget.generation||0,v.towLocal?.toArray()||[0,0,0],v.towLeg||'',v.towAngle||0,v.towWinding||0]));}
  return records;
}
export function applyWorldPresentation(game,record){
  if(!(record.payload instanceof Uint8Array)||record.payload.length>900)throw Error('Invalid world presentation');
  const a=JSON.parse(decode.decode(record.payload));if(!Array.isArray(a))throw Error('Invalid world presentation');
  if(record.schema===7){
    if(a.length!==11||!Number.isSafeInteger(a[0])||record.id!==200000+a[0]||!['built','unbuilt','destroyed'].includes(a[1])||a.slice(3,4).concat(a.slice(5,9)).some(n=>!Number.isFinite(n))||a[2]!==null&&!Number.isFinite(a[2])||![null,'idle','waiting','active'].includes(a[4])||typeof a[9]!=='boolean'||typeof a[10]!=='boolean')throw Error('Invalid building state');
    const b=game.live?.buildings[a[0]];if(!b||(game.turretSeats?.turrets||[]).includes(b))throw Error('Missing building prerequisite');
    if(b.state!==a[1])b.show(a[1]);b.health=a[2]??Infinity;b.team=a[3];
    for(const [i,key]of ['mode','active','pulse','facing','yaw','servedHealth','servedAmmo'].entries())b[key]=a[i+4];
    return true;
  }
  if(record.schema===CREATURE_STATE_SCHEMA){
    if(a.length!==8||a.some(n=>!Number.isFinite(n))||!Number.isSafeInteger(a[0])||record.id!==CREATURE_STATE_ID+a[0]||!Number.isSafeInteger(a[1])||a[1]<0||a[1]>4||a.slice(4,6).some(n=>!Number.isSafeInteger(n)||n<0)||a[6]<-1||a[6]>2)throw Error('Invalid creature state');
    const prop=game.live?.animated?.[a[0]];if(!prop?.grabs?.length)throw Error('Missing creature prerequisite');
    return prop.adopt(a.slice(1),game.units);
  }
  if(record.schema===TRAP_STATE_SCHEMA){
    if(a.length!==4||a.some(n=>!Number.isSafeInteger(n))||record.id!==TRAP_STATE_ID+a[0]||a[1]<0||a[1]>3||a[2]<0||a[3]<0||a[3]>1)throw Error('Invalid trap state');
    const trap=game.live?.traps?.[a[0]];if(!trap)throw Error('Missing trap prerequisite');
    return trap.adopt(a.slice(1));
  }
  if(record.schema!==8||a.length!==7||a.slice(0,3).some(n=>!Number.isSafeInteger(n)||n<0)||record.id!==300000+a[0]||!Array.isArray(a[3])||a[3].length!==3||a[3].some(n=>!Number.isFinite(n))||typeof a[4]!=='string'||a[4].length>128||a.slice(5).some(n=>!Number.isFinite(n)))throw Error('Invalid tow state');
  const vs=game.vehicles?.vehicles||[],v=vs.find(v=>v.aiTarget.id===a[0]&&v.aiTarget.generation===record.generation);if(!v)return false;
  const target=vs.find(v=>v.aiTarget.id===a[1]&&v.aiTarget.generation===a[2]);if(a[1]&&!target)return false;
  v.towTarget=target?vs.indexOf(target):-1;v.towLocal.fromArray(a[3]);v.towLeg=a[4];v.towAngle=a[5];v.towWinding=a[6];return true;
}
