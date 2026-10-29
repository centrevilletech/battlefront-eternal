import { Vector3 } from 'three';
import { emitSound, observeSounds } from '../audio/events.js';
import { networkShotFor } from './network-shot-aim.js';

export const PRESENTATION_SCHEMA=8;
const encoder=new TextEncoder(),decoder=new TextDecoder('utf-8',{fatal:true});
const kinds=new Set(['weaponAction','endWeaponAction','shot','remoteShot','shieldHit','deathBlast','blowApart','remoteBlast','effect','lightning','vo','bounce','pickup','serve','towAttach','towRelease','squad']);
const vector=v=>v?.toArray?.()??v??null;
const finite=n=>typeof n==='number'&&Number.isFinite(n);
const vec=v=>Array.isArray(v)&&v.length===3&&v.every(finite);
function validate(e){
  if(!e||!kinds.has(e.kind))throw Error('Invalid presentation kind');
  for(const [key,value]of Object.entries(e)){
    if(['kind','name','action','bank','posture','hitKind','line','reason','label','property'].includes(key)){if(value!==null&&(typeof value!=='string'||value.length>128))throw Error('Invalid presentation text');}
    else if(['position','normal','from','to','source'].includes(key)){if(value!==null&&!vec(value))throw Error('Invalid presentation vector');}
    else if(key==='channel'){if(value!==0&&value!==1)throw Error('Invalid presentation channel');}
    else if(['ownerId','ownerGeneration','mountId','mountGeneration','seat','fireId','definitionId','weaponId','building','team'].includes(key)){if(!Number.isSafeInteger(value)||value<0||value>0xffffffff)throw Error('Invalid presentation identity');}
    else if(['amount','charge','swing'].includes(key)){if(!finite(value))throw Error('Invalid presentation number');}
    else if(['end','prepare','repair','human','melee','positional'].includes(key)){if(typeof value!=='boolean')throw Error('Invalid presentation flag');}
    else throw Error('Unknown presentation field: '+key);
  }
  return e;
}
export function presentationRecord({id,generation,tick,event}){
  // Scripted kills use infinite damage. Debris already saturates its speed
  // at -50 health; send that finite visual value instead of losing the event.
  if(event?.kind==='blowApart')event={...event,amount:Math.max(-50,event.amount)};
  const payload=encoder.encode(JSON.stringify(validate(event)));if(payload.length>900)throw Error('Presentation event too large');
  return {kind:'combat',id,generation,tick,schema:PRESENTATION_SCHEMA,payload};
}
export function decodePresentation(record){
  if(record.kind!=='combat'||record.schema!==PRESENTATION_SCHEMA||!(record.payload instanceof Uint8Array)||record.payload.length>900)throw Error('Invalid presentation payload');
  const event=validate(JSON.parse(decoder.decode(record.payload)));
  return {...record,...event,kind:'combat',presentationKind:event.kind,expiryTick:record.tick+120};
}
export function attachPresentationCapture(game,capture,registry){
  const emit=event=>{try{capture.emit(presentationRecord({id:capture.nextId++,generation:capture.generation,tick:capture.tick(),event}));}catch(error){capture.fault(error);}};
  const describe=record=>{
    if(record.kind==='presentation')emit(record.event);
    if(record.kind==='vo'){
      const u=record.source?.unit;
      emit({kind:'vo',ownerId:u?.id||0,ownerGeneration:record.source?.generation||0,team:record.team||0,position:vector(record.position),human:!!record.human,property:record.property||'',positional:record.positional!==false});
    }
    if(record.kind==='shot'){
      const u=record.source?.unit;
      const mount=record.mount?.unit;
      const weapon=mount?.vehicle?.seats[record.seat]?.weapons.find(w=>w.state.name===record.weapon)?.state
        ??mount?.turret?.weapon??u?.combat?.arsenal.weapons.find(w=>w.name===record.weapon);
      const channel=weapon?.channel??0;
      emit({kind:'shot',ownerId:u?.id||0,ownerGeneration:record.source?.generation||0,mountId:mount?.id||0,mountGeneration:record.mount?.generation||0,seat:record.seat||0,fireId:networkShotFor(u,channel)?.id||0,channel,weaponId:registry.weaponIds.get(record.weapon)||0,position:vector(record.position),repair:!!record.repair,melee:!!record.melee,swing:record.swing||0});
    }
    // A recon droid's gun by the droid's class (in the registry) and the
    // gun's channel; the droid's own weapon classes are not registered.
    if(record.kind==='remoteShot')emit({kind:'remoteShot',definitionId:registry.combat.definitionId(record.droid),channel:record.channel===1?1:0,position:vector(record.position)});
  };
  // Runs inside the simulation's own emit: a record that cannot be described
  // (content absent from the registry) is a capture fault, never the tick's.
  const unlisten=game.simEvents.listen(record=>{try{describe(record);}catch(error){capture.fault(error);}});
  const unobserve=observeSounds((kind,...args)=>{
    if(kind==='bounce'||kind==='pickup'){
      let definitionId;try{definitionId=registry.combat.definitionId(args[0]);}catch{return;}
      emit({kind,definitionId,position:vector(args[1]),amount:args[2]||0});
    }else if(kind==='serve'){
      const building=game.live?.buildings.indexOf(args[0]);if(!(building>=0))return;
      // Serving is frequent. Its persistent mode drives the loop; one onset
      // per station per tick is enough and retains the stock sound throttle.
      const unit=[...game.units.all()].find(u=>u.supply===args[1]);
      emit({kind,building,ownerId:unit?.id||0,ownerGeneration:unit?.generation||0});
    }else if(kind==='towAttach'||kind==='towRelease'){
      if(!game.vehicles?.vehicles.includes(args[0]))return;
      emit({kind,ownerId:args[0].aiTarget.id,ownerGeneration:args[0].aiTarget.generation,weaponId:registry.weaponIds.get(args[1]?.class)||0,reason:args[2]||null});
    }
  });
  return()=>{unlisten();unobserve();};
}
export function presentEntityEvent(game,event,registry){
  const kind=event.presentationKind,u=event.ownerId?game.units.byId(event.ownerId):null;
  if(kind==='lightning'){
    // Drawn at once, as the world placed it. A bolt naming its weapon's
    // channel is held to the shooter: its start follows his gun origin as
    // this page draws him (a body this page doesn't know leaves it put).
    const def=game.effects.lightning?.[event.name];if(!def||!vec(event.from)||!vec(event.to))return false;
    const shooter=u&&u.generation===event.ownerGeneration?u.combat:null;
    game.lightning?.spawn(def,new Vector3(...event.from),new Vector3(...event.to),event.charge,event.channel!=null&&shooter?out=>shooter.drawnGunOrigin(out):null);
    return true;
  }
  if(event.ownerId&&(!u||u.generation!==event.ownerGeneration))return false;
  const combat=u?.combat,point=event.position?new Vector3(...event.position):null;
  if(kind==='effect'){if(!event.name||!point)return false;game.effects.play(event.name,point,event.normal?new Vector3(...event.normal):null);if(event.name==='headshot')game.ordnance?.presentHeadshot?.(point);}
  else if(kind==='weaponAction'||kind==='endWeaponAction'){
    if(!combat)return false;
    if(kind==='endWeaponAction')u.animator.endWeaponAction(event.action??undefined);
    else{
      if(!['reload','charge','shoot','shoot2','shoot3','shoot4','shoot5'].includes(event.action)||!['stand','crouch','prone'].includes(event.posture))throw Error('Invalid weapon animation');
      u.animator.setBank?.(event.bank,u.soldier.aimYaw);u.animator.weaponAction(event.action,event.bank,event.posture);
      if(event.prepare&&combat.firstPerson){const w=combat.arsenal.weapons.find(w=>w.bank===event.bank&&w.busy);if(w)combat.view?.play('prepareThrow',w);}
    }
  }else if(kind==='shot'){
    const name=registry.weapons.get(event.weaponId);
    const mount=event.mountId?game.units.byId(event.mountId):null;
    if(event.mountId&&(!mount||mount.generation!==event.mountGeneration))return false;
    const vehicle=mount?.vehicle,turret=mount?.turret;
    const weapon=vehicle?.seats[event.seat]?.weapons?.find(w=>w.state.name===name)?.state||turret?.weapon||combat?.arsenal.weapons.find(w=>w.name===name);
    if(!weapon)return false;
    game.hud?.noteShot?.(mount||u,weapon.def);
    if(u&&game.predictedShots?.claim({playerId:u.id,generation:u.generation,fireId:event.fireId,channel:event.channel??0},{tick:event.tick}))return true;
    emitSound(mount?'turretFire':u===game.playerUnit?'fire':'actorFire',vehicle||turret||combat,{weapon,repair:event.repair});
    if(!mount&&event.repair)combat.presentRepairTick(weapon);
    else if(!mount&&!event.melee)combat.presentProjectileShot(weapon);
    else if(vehicle&&point){
      vehicle.kickBarrel?.(weapon,point);
      if(weapon.def.MuzzleFlashEffect)game.effects.play(weapon.def.MuzzleFlashEffect,point);
      game.ordnance.lights.flash(point,[1,.55,.2],5,.06);
    }
    if(event.melee)u?.animator.fire?.();
  }else if(kind==='remoteShot'){
    const droid=registry.combat.definitions.get(event.definitionId);
    const def=droid?.base==='droid'?(droid.weapons||[]).find(w=>(w.channel??0)===(event.channel??0)):null;
    if(!def||!point)return false;
    game.ordnance.lights.flash(point,[1,.75,.75],def.FlashLightRadius||0,def.FlashLightDuration||0);
  }else if(kind==='vo'){
    game.audio.on('vo',{source:u?{unit:u,generation:u.generation}:null,team:event.team,position:event.position,human:u===game.playerUnit,property:event.property,positional:u===game.playerUnit?event.positional:true});
  }else if(kind==='squad'){if(u===game.playerUnit)game.hud.squadCommand(event.label);}
  else if(['shieldHit','deathBlast','blowApart','remoteBlast'].includes(kind))game.presentEvent(kind,kind==='remoteBlast'?point:u,event.amount||0,event.source?new Vector3(...event.source):null,event.hitKind||null);
  else if(kind==='bounce'||kind==='pickup'){
    const def=registry.combat.definitions.get(event.definitionId);if(!def||!point)return false;emitSound(kind,def,point,event.amount);
  }else if(kind==='serve'){
    const station=game.live?.buildings[event.building];if(!station)return false;emitSound('serve',station,u?{...u.supply,player:u===game.playerUnit}:null);
  }else if(kind==='towAttach'||kind==='towRelease'){
    const v=u?.vehicle,name=registry.weapons.get(event.weaponId),w=v?.seats.flatMap(s=>s.weapons).find(w=>w.state.name===name);if(v&&w)emitSound(kind,v,w.def,event.reason);
  }
  return true;
}
