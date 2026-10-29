import { damageDirection } from '../combat-input.js';
import { radarRelation } from '../hud-units.js';
// Damage/death notifications never apply health. Health and dead state remain
// independently replicated actor state; losing this record cannot revive a body.
export function feedbackRecord({id,generation,tick,event}){
 const schema=event.kind==='damage'?3:event.kind==='death'?4:0;if(!schema)throw Error('unsupported combat feedback');
 const payload=new Uint8Array(78),v=new DataView(payload.buffer);let at=0;
 for(const n of [tick+18,event.target?.unit?.id??0,event.target?.generation??0,event.attacker?.unit?.id??0,event.attacker?.generation??0]){if(!Number.isSafeInteger(n)||n<0||n>0xffffffff)throw Error('invalid feedback identity');v.setUint32(at,n,true);at+=4;}
 for(const n of event.position){if(!Number.isFinite(n))throw Error('invalid feedback position');v.setFloat64(at,n,true);at+=8;}
 v.setInt32(at,event.team??-1,true);at+=4;v.setInt32(at,event.attackerTeam??-1,true);at+=4;const source=event.source;
 const hasSource=Array.isArray(source)&&source.length===3&&source.every(Number.isFinite);
 v.setUint16(at,hasSource?1:0,true);at+=2;
 for(const n of hasSource?source:[0,0,0]){v.setFloat64(at,n,true);at+=8;}
 return{kind:'combat',id,generation,tick,schema,payload};
}
export function decodeFeedback(record){
 if(![3,4].includes(record.schema)||record.payload?.length!==78)throw Error('invalid feedback payload');
 const v=new DataView(record.payload.buffer,record.payload.byteOffset,record.payload.byteLength);let at=0;const u=()=>{const n=v.getUint32(at,true);at+=4;return n;};
 const result={...record,expiryTick:u(),targetId:u(),targetGeneration:u(),attackerId:u(),attackerGeneration:u(),position:[]};
 for(let i=0;i<3;i++){const n=v.getFloat64(at,true);at+=8;if(!Number.isFinite(n))throw Error('invalid feedback position');result.position.push(n);}
 result.team=v.getInt32(at,true);at+=4;result.attackerTeam=v.getInt32(at,true);at+=4;
 const flags=v.getUint16(at,true);at+=2;if(flags>1)throw Error('invalid feedback flags');
 const source=[];for(let i=0;i<3;i++){const n=v.getFloat64(at,true);at+=8;if(!Number.isFinite(n))throw Error('invalid feedback source');source.push(n);}
 result.source=flags?source:null;
 if(result.expiryTick!==record.tick+18)throw Error('invalid feedback expiry');return result;
}
// Whether a decoded feedback record can matter to the page whose own unit
// is unitId: presentFeedback only acts on damage (schema 3) dealt or taken
// by the own unit, while a death (schema 4) feeds every kill message. The
// server sends damage to those two pages only.
export function feedbackConcerns(event,unitId){
 return event.schema!==3||(unitId!==undefined&&unitId!==null&&(event.attackerId===unitId||event.targetId===unitId));
}
export function presentFeedback(game,event){
 const target=game.units.byId(event.targetId),attacker=game.units.byId(event.attackerId),own=game.playerUnit;
 const targetMatches=target?.generation===event.targetGeneration,attackerMatches=attacker?.generation===event.attackerGeneration;
 // Who hurt the page's own player, and on his death who killed him: the
 // death camera turns to that one (game.js noteAttacker). No attacker is
 // null; one this page doesn't have is undefined.
 if(targetMatches&&target===own)game.noteAttacker?.(attackerMatches?attacker:event.attackerId?undefined:null,event.schema===4,event.tick);
 if(event.schema===3){
  // A sniper hit this page already marked when its round landed (game.js).
  if(attackerMatches&&attacker===own&&!game.claimPredictedHit?.(event.targetId))game.hud.hit(game.units.relation(event.attackerTeam,event.team));
  if(targetMatches&&target===own&&event.source){
   game.hud.damageFrom(damageDirection(event.source[0]-event.position[0],event.source[2]-event.position[2],own.soldier.aimYaw));
  }
 }else if(targetMatches){
  game.hud.killMessage({killer:attackerMatches?attacker:null,victim:target,relationToViewer:radarRelation(own.team,event.attackerTeam,(a,b)=>game.units.relation(a,b),team=>game.units.locals?.(team)??team>2)});
 }
}
