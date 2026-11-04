// Calls outside the vehicle package, on the existing live objects. Each
// packet is copied before decoding because callbacks can reenter the ABI.
import { Vector3 } from 'three';
import { agentWorld } from './live-agent-world.js';
import { encode, decoder, shapes } from './wire.js';
import { unitView,hullView,turretView,leaseView,controllerView,requestView,requestObject,steerView,writeSteer,exitOptions } from './live-views.js';
export function vehicleWorld(service, sim, args, frame) {
  const op=args.op, values=[...args.values];
  const d=decoder(values,sim),read=d.read,id=o=>sim.id(o),objects=sim.objects;
  const obj=()=>objects[read('Obj')], optionalObj=()=>{const n=read(['option','Obj']);return n==null ? null:objects[n];};
  const vector=()=>{const v=read('Vector3');return new Vector3(v.x,v.y,v.z);};
  const ai=service.ai,world=service.world,system=service.system,turrets=service.turrets,port=service.port;
  const notifyRelation=(a,b)=>world.relation ? world.relation(a,b) : world.units?.relation ? world.units.relation(a,b) : a===b ? (a?1:0) : !a||!b ? 0:-1;
  const reply=(type,value)=>{d.done();return {values:type ? encode(type,value,sim):[]};};
  const nothing=()=>reply(null);
  switch(op) {
    case 'settings': {const h=service.flyHeights();return reply('Settings',{min_fly_height:h.min,max_fly_height:h.max,notify_radius:service.notifyRadius(),dense:service.dense(),stay_in_turrets:Boolean(ai.stayInTurrets?.()),cycle:ai.schedulerNumbers?.()?.cycle ?? (ai.isHard?.()?3:15)});}
    case 'nextA': return reply('u32',ai.randomA.next());
    case 'now': return reply('f64',world.time ?? 0);
    case 'unit': return reply('UnitView',unitView(obj(),id));
    case 'hull': return reply('HullView',hullView(obj(),id,service));
    case 'turret': return reply('TurretView',turretView(obj(),id));
    case 'lease': return reply('LeaseView',leaseView(obj(),id));
    case 'controller': return reply('ControllerView',controllerView(obj(),id));
    case 'post': {const p=obj();return reply('PostView',{obj:id(p),position:p.position,team:p.team});}
    case 'characterUnit': {const c=obj();return reply(['option','Obj'],c.unit ? id(c.unit):null);}
    case 'characterController': {const c=obj();return reply(['option','Obj'],c.controller ? id(c.controller):null);}
    case 'hulls': return reply(['list','Obj'],(system?.vehicles ?? []).map(id));
    case 'turrets': return reply(['list','Obj'],(turrets?.turrets ?? []).map(id));
    case 'weapons': {const u=obj(),ws=u.combat?.arsenal?.weapons;return reply(['option',['list','WeaponView']],ws ? ws.map(w=>({base:w.def?.base ?? null,channel:w.channel})):null);}
    case 'reinforcements': {const team=read('f64');return reply(['option','f64'],world.battle?.reinforcements?.[team] ?? null);}
    case 'humanUnits': return reply(['list','Obj'],[...(ai.humanUnits?.() ?? [])].map(id));
    case 'hasPlanner': return reply('bool',Boolean(world.nav?.planner));
    case 'hasGraph': return reply('bool',Boolean(world.nav?.graph));
    case 'supportHolds': return reply(['option','f64'],obj().supportHolds ?? null);
    case 'hintLink': {const h=obj();return reply(['option','Obj'],h.link ? id(h.link):null);}
    case 'readSteer': return reply(['option','Steer'],steerView(obj().steer,id,shapes.Steer));
    case 'writeSteer': {const u=obj(),s=read(['option','Steer']);writeSteer(u,s,objects);return nothing();}
    case 'readDecay': return reply(['option','Decay'],obj().decay ?? null);
    case 'writeDecay': {const h=obj(),s=read('Decay');Object.assign(h.decay ??= {},s);return nothing();}
    case 'writeVehicleIntent': {const u=obj(),s=read('SeatIntent');if(u.intent)Object.assign(u.intent,s);return nothing();}
    case 'cast': {const origin=vector(),direction=vector(),far=read('f64'),ignore=optionalObj();
      const hit={distance:0,point:new Vector3(),normal:new Vector3(),terrain:false,body:null};
      const ok=world.collision?.cast(origin,direction,far,hit,ignore);
      return reply(['option','Hit'],ok ? {...hit,body:hit.body ? id(hit.body):null}:null);}
    case 'query': {const center=vector(),radius=read('f64'),kind=read('f64'),source=obj(),team=read('f64');
      const filter=u=>u!==source && (kind===2 ? notifyRelation(team,u.team)>=0 : u.alive && service.relation(team,u.team)<0 && (kind===1 || u.targetable!==false));
      const found=world.units?.query?.(center,radius,[],filter) ?? [];return reply(['list','Obj'],found.map(id));}
    case 'relation': {const a=read('f64'),b=read('f64');return reply('f64',ai.relation?.(a,b) ?? (a===b?1:!a||!b?0:-1));}
    case 'notifyRelation': {const a=read('f64'),b=read('f64');return reply('f64',notifyRelation(a,b));}
    case 'los': {const a=vector(),b=vector();return reply(['option','bool'],world.los ? Boolean(world.los(a,b)):null);}
    case 'sphere': {const s=port.sphere(obj());return reply('Sphere',{center:s.center,radius:s.radius});}
    case 'origin': {const u=obj();return reply('Vector3',u.origin(new Vector3()));}
    case 'forward': {const u=obj();return reply('Vector3',u.forward(new Vector3()));}
    case 'mountOrigin': {const kind=read('f64'),o=obj(),index=read('usize'),v=new Vector3();return reply('Vector3',kind===0 ? o.origin(v):kind===1 ? port.turretPivot(o,v):o.seatOrigin(index,v));}
    case 'holdYaw': {const t=obj(),a=read('f64');return reply('f64',t.holdYaw(a));}
    case 'eligibleSeat': {const u=obj(),h=obj();return reply('f64',system.eligibleSeat(u,h));}
    case 'firstFreeSeat': return reply('f64',obj().firstFreeSeat());
    case 'board': {const u=obj(),h=obj();return reply('bool',Boolean(system?.board?.(u,h)));}
    case 'man': {const u=obj(),t=obj();return reply('bool',Boolean(turrets?.man?.(u,t)));}
    case 'requestExitVehicle': case 'requestExitTurret': case 'unboard': {
      const u=optionalObj(),options=exitOptions(read('ExitOptions'),objects);
      const host=op==='requestExitTurret'?turrets:system,method=op==='unboard'?'unboard':'requestExit';return reply('bool',Boolean(host?.[method]?.(u,options)));}
    case 'humanTarget': {const h=obj(),target=system?.humanTarget?.(h);return reply(['option','Obj'],target ? id(target):null);}
    case 'damage': {const h=obj(),amount=read('f64');h.damage(amount);return nothing();}
    case 'pilotInput': {const u=obj(),r=requestObject(read(['option','Request'])),a=read(['option','Vector3']),dt=read('f64'),e=read('PilotEnv');
      const collision=world.collision,water=world.water ?? system?.water,ground={height:0,normal:new Vector3()};
      const env={now:world.time ?? 0,nav:world.nav ?? null,collision:collision ?? null,hasPath:e.has_path,
        minFlyHeight:e.min_fly_height,maxFlyHeight:e.max_fly_height,
        height:collision?.ground ? (x,z)=>collision.ground(x,z,10000,ground)?ground.height:-Infinity:null,
        water:water ? (x,z)=>water.over?.(x,z)?water.level:null:null,dangerCrosses:(a,b,r)=>service.dangerCrosses(a,b,r)};
      const out={forward:0,right:0,turn:0,pitch:0,up:0,down:0,brake:false,trick:false,immelmann:false,boost:false};
      port.pilotInput(u,r,a ? new Vector3(a.x,a.y,a.z):null,dt,env,out);return reply('Stick',out);}
    case 'navAttach': {const c=optionalObj(),u=obj();(frame?.control ?? c?.navControl)?.attach?.(u);return nothing();}
    case 'navUpdate': {const c=optionalObj(),dt=read('f64');(frame?.control ?? c?.navControl)?.update?.(dt);return nothing();}
    case 'navRequest': {const c=optionalObj();return reply(['option','Request'],requestView((frame?.control ?? c?.navControl)?.request));}
    case 'reserveHint': {const h=obj(),c=obj(),now=read('f64');ai.nav?.hints?.reserve?.(h,c,now);return nothing();}
    case 'setHintReservedAt': {const h=obj(),at=read('f64');h.reservedAt=at;return nothing();}
    case 'setIntentAnalogs': {const u=obj(),s=read('Stick');
      Object.assign(service.seatOut,{forward:s.forward,right:s.right,turn:s.turn,pitch:s.pitch});
      Object.assign(frame?.intent ?? u.intent,service.seatOut);return nothing();}
    case 'requestObjective': {const c=obj(),exclude=read('bool');c.requestObjective?.(exclude);return nothing();}
    case 'controllerRequestExit': obj().requestExit();return nothing();
    case 'setReboardAt': {const c=obj(),t=read('f64');c.reboardAt=t;return nothing();}
    case 'fireA': obj().fireA=true;return nothing();
    case 'invalidateRequest': obj().request.valid=false;return nothing();
    case 'addVehicleNote': {const c=obj(),r=read('Ref');c.addVehicleNote?.({unit:objects[r.obj],generation:r.generation});return nothing();}
    case 'setObjective': {const c=obj(),p=obj();c.objective=p;return nothing();}
    case 'setSupportHolds': {const a=obj(),n=read('f64');a.supportHolds=n;return nothing();}
    case 'setPatrolExpires': {const p=obj(),t=read('f64');p.expires=t;return nothing();}
    case 'patrolRequestState': {const p=obj(),s=read('f64');p.requestState(s);return nothing();}
    case 'cpCenter': return reply('Vector3',port.cpCenter(ai,obj(),new Vector3()));
    case 'captureRegion': {const r=port.captureRegion(ai,obj());return reply(['option',['array','f64',2]],r ? [r.sizeX,r.sizeZ]:null);}
    case 'helperTraversal': {const h=obj(),radius=read('bool');return reply('f64',radius?h.radius:h.mask);}
    case 'memoryPoint': {const a=obj();return reply(['option','Vector3'],ai.combat?.memoryPoint?.(a) ?? null);}
    case 'selectWeapon': {const c=obj(),u=obj(),target=obj(),allowed=read('bool'),out={};const ok=port.selectWeapon(c,u,target,allowed,world,out);
      return reply('Choice',{ok:Boolean(ok.ok),action:out.action,in_band:out.inBand,weapon:out.weapon ? id(out.weapon):null});}
    case 'switchWeapon': {const c=obj(),u=obj(),i=read('usize');return reply('bool',Boolean(port.switchWeapon(c,u,i)));}
    case 'face': {const p=obj(),posture=read('f64');p.face(posture);return nothing();}
    case 'setDelay': {const a=obj(),seconds=read('f64'),now=read('f64');a.tracker?.setDelay(seconds,now);return nothing();}
    case 'defaultDelay': {const a=obj(),now=read('f64');a.tracker?.defaultDelay(now);return nothing();}
    case 'helperGoto': {const a=obj(),p=vector(),direct=read('bool');if(direct)a.helper?.goto?.(p,{mode:'direct'});else a.helper?.goto?.(p);return nothing();}
    case 'helperStop': obj().helper?.stop?.();return nothing();
    case 'helperFollowPath': {const a=obj(),points=read(['list',['array','f64',3]]),over=read('bool'),altitude=read('bool');a.helper?.followPath?.(points,over,altitude);return nothing();}
    case 'helperLand': {const a=obj(),on=read('bool');a.helper?.land?.(on);return nothing();}
    default: return agentWorld(service,sim,op,values);
  }
}
