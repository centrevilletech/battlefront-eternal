import { presentationModule } from '../presentation/module.js';
import { Vector3 } from 'three';
import { emitSound, observeSounds } from '../audio/events.js';
import { combatBridge, decodeFields, allocateCapture } from './network-entity-combat.js';
import { createWirePresentationBridge } from './wire-presentation.js';

export const PRESENTATION_SCHEMA=8;
const decoder=new TextDecoder('utf-8',{fatal:true}),plans=new WeakMap();
const vector=v=>v?.toArray?.()??v??null;
const native=()=>presentationModule().module.PageCombat;
let encoder;
export function presentationRecord({id,generation,tick,event,native:service=null}){
  return {kind:'combat',id,generation,tick,schema:PRESENTATION_SCHEMA,payload:(service??(encoder??=createWirePresentationBridge(combatBridge()))).event(event)};
}
export function decodePresentation(record){
  const numeric=decodeFields(record),event=JSON.parse(decoder.decode(record.payload));
  return {id:record.id,generation:record.generation,tick:record.tick,schema:record.schema,payload:record.payload,...event,...numeric,kind:'combat',presentationKind:event.kind};
}
// Enumerate resource candidates without selecting one. Native queries retain
// vehicle-seat, turret and infantry precedence and return opaque indices.
function weapons(u,m){
  const rows=[];
  for(const [seat,body]of(m?.vehicle?.seats??[]).entries())for(const w of body.weapons??[])rows.push({state:w.state,role:'vehicle',seat});
  if(m?.turret?.weapon)rows.push({state:m.turret.weapon,role:'turret'});
  for(const state of u?.combat?.arsenal?.weapons??[])rows.push({state,role:'combat'});
  return rows;
}
const facts=rows=>rows.map(({state,role,seat})=>({name:state.name,channel:state.channel,bank:state.bank,busy:!!state.busy,flash:state.def?.MuzzleFlashEffect,role,seat}));
export function attachPresentationCapture(game,capture,registry,service=null){
  const emit=event=>{if(event)capture.emit(presentationRecord({id:allocateCapture(capture),generation:capture.generation,tick:capture.tick(),event,native:service}));};
  const describe=r=>{
    if(r.kind==='presentation'){emit(r.event);return;}
    const u=r.source?.unit,m=r.mount?.unit;
    emit(JSON.parse(native().event_capture(JSON.stringify({kind:r.kind,ownerId:u?.id,ownerGeneration:r.source?.generation,mountId:m?.id,mountGeneration:r.mount?.generation,seat:r.seat,name:r.weapon,weapons:facts(weapons(u,m)),shots:u?.networkShots?.map(edge=>({button:edge.button,id:edge.id})),fireId:u?.networkShot?.id,weaponId:registry.weaponIds?.get(r.weapon),position:vector(r.position),repair:r.repair,melee:r.melee,swing:r.swing,team:r.team,human:r.human,property:r.property,positional:r.positional,channel:r.channel,definitionId:r.kind==='remoteShot'?registry.combat.definitionId(r.droid):undefined}))));
  };
  const guard=fn=>{try{fn();}catch(error){capture.fault(error);}};
  const unlisten=game.simEvents.listen(r=>guard(()=>describe(r)));
  const unobserve=observeSounds((kind,...args)=>guard(()=>{
    let f;
    switch(kind){
      case 'bounce':case 'pickup':
        try{f={definitionId:registry.combat.definitionId(args[0]),position:vector(args[1]),amount:args[2]};}catch{return;}break;
      case 'serve':f={buildings:Array.from(game.live?.buildings??[],row=>row===args[0]),units:Array.from(game.units.all(),u=>({id:u.id,generation:u.generation,supply:u.supply===args[1]}))};break;
      case 'towAttach':case 'towRelease':f={vehicles:Array.from(game.vehicles?.vehicles??[],row=>row===args[0]),ownerId:args[0]?.aiTarget?.id,ownerGeneration:args[0]?.aiTarget?.generation,weaponId:registry.weaponIds.get(args[1]?.class),reason:args[2]??null};break;
      default:return;
    }
    emit(JSON.parse(native().event_capture(JSON.stringify({kind,...f}))));
  }));
  return()=>{unlisten();unobserve();};
}
export function presentEntityEvent(game,event,registry){
  const k=native(),u=event.ownerId?game.units.byId(event.ownerId):null,m=event.mountId?game.units.byId(event.mountId):null,combat=u?.combat;
  if(!k.event_ready(event.presentationKind,event.ownerId??0,event.ownerGeneration??0,!!u,u?.generation??0,event.mountId??0,event.mountGeneration??0,!!m,m?.generation??0))return false;
  const lightning=game.effects?.lightning?.[event.name],def=registry?.combat?.definitions?.get(event.definitionId),station=game.live?.buildings[event.building];
  let saved=plans.get(event);
  if(!saved||saved.u!==u||saved.m!==m||saved.def!==def||saved.lightning!==lightning||saved.station!==station||saved.combat!==combat||saved.vehicle!==m?.vehicle||saved.turret!==m?.turret){
    const rows=weapons(u,m),tow=[];for(const seat of u?.vehicle?.seats??[])for(const w of seat.weapons)tow.push(w);
    let plan;try{plan=JSON.parse(k.event_plan(JSON.stringify(event),JSON.stringify({owner:!!u,generation:u?.generation,own:u===game.playerUnit,combat:!!combat,firstPerson:!!combat?.firstPerson,mount:!!m,vehicle:!!m?.vehicle,turret:!!m?.turret,ownerVehicle:!!u?.vehicle,name:registry?.weapons?.get(event.weaponId),weapons:facts(rows),definition:!!def,lightning:!!lightning,base:def?.base,droidWeapons:def?.weapons?.map(w=>({channel:w.channel,FlashLightRadius:w.FlashLightRadius,FlashLightDuration:w.FlashLightDuration})),station:!!station,towWeapons:tow.map(w=>({name:w.state.name}))})));}catch(error){throw Error(error);}
    saved={u,m,def,lightning,station,combat,vehicle:m?.vehicle,turret:m?.turret,rows,tow,...plan};plans.set(event,saved);
  }
  if(!saved.accepted)return false;
  const point=event.position?new Vector3(...event.position):null,normal=event.normal?new Vector3(...event.normal):null,source=event.source?new Vector3(...event.source):null;
  for(const [op,a,b,c]of saved.commands){const weapon=saved.rows[a]?.state;
    switch(op){
      case 'lightning':{let follow=null;if(a){follow=out=>combat.drawnGunOrigin(out);follow.unit=u;}game.lightning?.spawn(lightning,new Vector3(...event.from),new Vector3(...event.to),event.charge,follow);break;}
      case 'effect':game.effects.play(event.name,point,normal);break;
      case 'headshot':game.ordnance?.presentHeadshot?.(point);break;
      case 'endAction':u.animator.endWeaponAction(event.action??undefined);break;
      case 'action':u.animator.setBank?.(event.bank,u.soldier.aimYaw);u.animator.weaponAction(event.action,event.bank,event.posture);break;
      case 'prepare':combat.view?.play('prepareThrow',weapon);break;
      case 'hud':game.hud?.noteShot?.(m||u,weapon.def);break;
      case 'claim':if(game.predictedShots?.claim({playerId:u.id,generation:u.generation,fireId:event.fireId,channel:event.channel??0},{tick:event.tick}))return true;break;
      case 'sound':emitSound(b,[m?.vehicle,m?.turret,combat][c],{weapon,repair:event.repair});break;
      case 'repair':combat.presentRepairTick(weapon);break;
      case 'projectile':combat.presentProjectileShot(weapon);break;
      case 'kick':m.vehicle.kickBarrel?.(weapon,point);break;
      case 'flashEffect':game.effects.play(a,point);break;
      case 'flash':game.ordnance.lights.flash(point,a,b,c);break;
      case 'melee':u?.animator.fire?.();break;
      case 'vo':game.audio.on('vo',{source:u?{unit:u,generation:u.generation}:null,team:event.team,position:event.position,human:a,property:event.property,positional:b});break;
      case 'squad':game.hud.squadCommand(event.label);break;
      case 'present':game.presentEvent(event.presentationKind,a?point:u,b,source,c);break;
      case 'objectSound':emitSound(event.presentationKind,def,point,event.amount);break;
      case 'serve':emitSound('serve',station,u?{...u.supply,player:a}:null);break;
      case 'tow':emitSound(event.presentationKind,u.vehicle,saved.tow[a].def,event.reason);break;
    }
  }
  return true;
}
