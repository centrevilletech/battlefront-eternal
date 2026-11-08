import { combatEncoder, decodeCombat } from "./network-entity-combat.js";
import { presentationModule } from '../presentation/module.js';
import { FeedbackInput as FI, FeedbackOutput as FO } from '../presentation/ordnance-layout.js';
let loaded,kernel,buffer,input,output;
function nativeFeedback() {
 loaded??=presentationModule();kernel??=new loaded.module.PageCombat(0);
 const b=loaded.memory.buffer;
 if(b!==buffer){buffer=b;input=new Float64Array(b,kernel.feedback_input_ptr(),kernel.feedback_input_len());output=new Float64Array(b,kernel.feedback_output_ptr(),kernel.feedback_output_len());}
}
// Damage/death notifications never apply health. Health and dead state remain
// independently replicated actor state; losing this record cannot revive a body.
export function feedbackRecord({id,generation,tick,event,native=null}){return (native??combatEncoder()).feedback({id,generation,tick,event});}
export function decodeFeedback(record){return decodeCombat(record);}
// Whether a decoded feedback record can matter to the page whose own unit
// is unitId: presentFeedback only acts on damage (schema 3) dealt or taken
// by the own unit, while a death (schema 4) feeds every kill message. The
// server sends damage to those two pages only.
export function feedbackConcerns(event,unitId){
 return presentationModule().module.PageCombat.concerns(event.schema,event.targetId,event.attackerId,unitId??NaN);
}
export function presentFeedback(game,event){
 const target=game.units.byId(event.targetId),attacker=game.units.byId(event.attackerId),own=game.playerUnit;
 // Resource lookups and mission relations are host facts, submitted together.
 nativeFeedback();const a=input;a.fill(0);
 a[FI.SCHEMA]=event.schema;a[FI.TARGET_PRESENT]=+!!target;a[FI.ATTACKER_PRESENT]=+!!attacker;
 a[FI.TARGET_GENERATION]=event.targetGeneration;a[FI.ATTACKER_GENERATION]=event.attackerGeneration;
 a[FI.TARGET_ACTUAL_GENERATION]=target?.generation??0;a[FI.ATTACKER_ACTUAL_GENERATION]=attacker?.generation??0;
 a[FI.TARGET_OWN]=+(target===own);a[FI.ATTACKER_OWN]=+(attacker===own);a[FI.ATTACKER_ID]=event.attackerId;
 a[FI.HAS_SOURCE]=+!!event.source;a[FI.YAW]=own?.soldier?.aimYaw??0;
 a[FI.OWN_TEAM]=own?.team??0;a[FI.ATTACKER_TEAM]=event.attackerTeam;
 const relation=game.units.relation?.(own?.team,event.attackerTeam)??0;
 a[FI.RELATION]=typeof relation==='number'?relation:loaded.module.PageCombat.feedback_relation(relation);
 a[FI.LOCALS]=+loaded.module.PageCombat.feedback_locals(Number(game.units.locals?.(event.attackerTeam)??NaN),event.attackerTeam);
 if(event.source){a[FI.SOURCE]=event.source[0];a[FI.SOURCE+1]=event.source[1];a[FI.SOURCE+2]=event.source[2];}
 if(event.position){a[FI.POSITION]=event.position[0];a[FI.POSITION+1]=event.position[1];a[FI.POSITION+2]=event.position[2];}
 kernel.feedback();
 const note=output[FO.NOTE_ATTACKER],who=output[FO.ATTACKER_KIND],hit=output[FO.HIT],damage=output[FO.DAMAGE],death=output[FO.DEATH],angle=output[FO.ANGLE],viewerRelation=output[FO.RELATION];
 // Rust's decisions only drive notifications; replicated health stays separate.
 if(note)game.noteAttacker?.(who===2?attacker:who===1?undefined:null,event.schema===4,event.tick);
 if(hit&&!game.claimPredictedHit?.(event.targetId))game.hud.hit(game.units.relation(event.attackerTeam,event.team));
 if(damage)game.hud.damageFrom(angle);
 if(death)game.hud.killMessage({killer:who===2?attacker:null,victim:target,relationToViewer:RELATIONS[viewerRelation]});
}
const RELATIONS=['neutral','friendly','enemy','locals'];
