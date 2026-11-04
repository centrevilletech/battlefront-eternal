// Awareness uses the same AI instance and preserves host query/notification
// order while geometry and recipient iteration run in the module.
import { selectedWeapon, weaponAI } from './ai/classic/weapons-ai.js';
import { Vector3 } from 'three';

export class NativeAwareness {
  constructor(sim) {
    this.sim=sim;this.frames=[];this.heard=[];this.near=[];this.danger=[];
    sim.bridge.register({aiAwareness:args=>{
      const op=args.op,id=args.id,number=args.number,other=args.other,point=[...args.point],frame=this.frames[this.frames.length-1];
      const source=frame.source,result={flag:false,number:0,ids:[],values:[]};
      if(op==='alive') result.flag=Boolean(source?.alive);
      else if(op==='motion') {const v=source.velocity;result.values=[Number(Boolean(v)),v?v.x:0,v?v.y:0,v?v.z:0,source.kind];}
      else if(op==='weapon') {
        frame.weapon=source.combat?.arsenal?selectedWeapon(source,source.controller)??source.combat.arsenal.current(0):null;
        result.flag=Boolean(frame.weapon);
      } else if(op==='active') {const w=frame.weapon;result.flag=Boolean(w.charging||w.busy||w.wait>1e-6);}
      else if(op==='mute') {const sounds=frame.sounds;result.number=sounds?.(frame.weapon)??frame.weapon.def?.MuteDistance??400;}
      else if(op==='query') {
        const units=frame.units=frame.world.units,at=frame.at=source.position;
        units.query(at,number,this.heard,u=>u!==source && units.relation(source.team,u.team)<0);
        result.ids=this.heard.map(u=>sim.id(u));
      } else if(op==='recipient') {
        const u=sim.objects[id],c=frame.controller=u.controller;result.flag=Boolean(c?.addAcoustic);
        if(result.flag){const at=frame.at,p=u.position;result.values=[at.x,p.x,at.y,p.y,at.z,p.z];}
      } else if(op==='notify') frame.controller.addAcoustic({unit:source,generation:source.generation});
      else if(op==='livingStart') {
        if(number===0)frame.units=frame.world.units;
        frame.iterator=frame.units.living()[Symbol.iterator]();
      } else if(op==='livingNext') {
        const next=frame.iterator.next();result.flag=!next.done;
        if(result.flag){result.ids=[sim.id(next.value)];if(number===0)result.number=Number(Boolean(next.value.human));}
      } else if(op==='clear') {const fire=sim.objects[id].controller?.perception?.fire;if(fire)fire.protected.length=0;}
      else if(op==='protectQuery') {
        const human=sim.objects[id],units=frame.units;
        units.query(human.position,50,this.near,u=>u!==human && units.relation(human.team,u.team)>=0);
        result.ids=this.near.map(u=>sim.id(u));
      } else if(op==='protectRecipient') {
        const fire=frame.fire=sim.objects[id].controller?.perception?.fire;result.flag=Boolean(fire);if(fire)result.number=fire.protected.length;
      } else if(op==='protectAppend') {const human=sim.objects[id];frame.fire.protected.push({unit:human,generation:human.generation});}
      else if(op==='dangerRow') {
        const row=frame.world.difficulty?.dangerRow?.();result.flag=row!=null;
        if(result.flag){const [lo,hi]=row;frame.bounds=[lo,hi];result.values=[lo,hi];}
      } else if(op==='dangerDraw') {
        const lo=id===1?frame.bounds[0]:number,hi=id===1?frame.bounds[1]:other;
        frame.delay=(frame.world.randomA??frame.world.random).range(lo,hi);result.flag=typeof frame.delay==='number';if(result.flag)result.number=frame.delay;
      }
      else if(op==='dangerSkill'){result.flag=typeof frame.enemySkill==='number';if(result.flag)result.number=frame.enemySkill;}
      else if(op==='dangerSkillCompare')result.flag=frame.enemySkill<number;
      else if(op==='dangerTime') result.number=frame.world.time??0;
      else if(op==='dangerFirst'){frame.dangerScalar=frame.state.firstContact;result.flag=typeof frame.dangerScalar==='number'&&typeof frame.delay==='number';if(result.flag)result.number=frame.dangerScalar;}
      else if(op==='dangerLast'){frame.dangerScalar=frame.state.lastNotify;result.flag=typeof frame.dangerScalar==='number';if(result.flag)result.number=frame.dangerScalar;}
      else if(op==='dangerAdd')result.number=frame.dangerScalar+(id===0?frame.delay:1);
      else if(op==='dangerQuery') {frame.world.units.query(frame.position,7.5,this.danger,u=>u!==frame.owner);result.ids=this.danger.map(u=>sim.id(u));}
      else if(op==='dangerRecipient') {const u=frame.recipient=sim.objects[id],c=frame.controller=u.controller;result.flag=Boolean(c||u.human);if(result.flag){const kind=u.kind;result.number=typeof kind==='number'?kind:NaN;}}
      else if(op==='dangerWarn') {const onWarn=frame.onWarn;onWarn?.(frame.recipient);}
      else if(op==='dangerHasNotify') result.flag=Boolean(frame.controller?.notifyDanger);
      else if(op==='dangerNotify') frame.controller.notifyDanger(frame.position);
      else if(op==='dangerWrite') frame.state.lastNotify=number;
      else if(op==='shotStart') result.flag=Boolean(frame.shooter && frame.target?.alive);
      else if(op==='shotSource') {frame.source=frame.shooter.character??null;result.flag=Boolean(frame.source);}
      else if(op==='shotRelation') result.number=frame.world.units.relation(frame.shooter.team,frame.target.team);
      else if(op==='shotHuman') result.flag=Boolean(frame.target.human);
      else if(op==='damageStart') {
        result.flag=Boolean(frame.target?.alive && frame.attacker);
        if(result.flag){frame.source=frame.attacker.character??frame.attacker;frame.team=frame.attacker.team??frame.source.team;}
      } else if(op==='damageRelation') result.number=frame.world.units.relation(frame.target.team,frame.team);
      else if(op==='damagePacket') result.values=[frame.packet?.amount??0,frame.packet?.generic??1,frame.packet?.typed??1];
      else if(op==='alertSeat') {
        const lease=frame.lease=frame.target.seat;result.values=[Number(Boolean(lease)),Number(Boolean(lease?.human)),lease&&!lease.human?sim.id(lease.controllable?.controller):0];
      } else if(op==='alertSeatController') result.number=sim.id(frame.lease.controllable.controller);
      else if(op==='alertOwn') result.number=sim.id(frame.target.controller);
      else if(op==='alertCanNotify') result.flag=Boolean(sim.objects[id]?.notifyShot);
      else if(op==='alertBody') {frame.body=frame.target.turret??frame.target.vehicle??null;result.flag=Boolean(frame.body&&!frame.target.lease);}
      else if(op==='alertLeases') {const body=frame.body;result.ids=(Array.isArray(body.seats)?body.seats.map(s=>s.lease):[body.lease]).map(l=>sim.id(l));}
      else if(op==='alertLease') {const lease=sim.objects[id];result.values=[Number(Boolean(lease.human)),Number(Boolean(lease.released)),Number(lease.live===false)];}
      else if(op==='alertCrew') {const lease=sim.objects[id];result.number=sim.id(lease.controllable?.controller??lease.unit?.controller);}
      else if(op==='alertNotify') sim.objects[id].notifyShot(frame.source,frame.known,frame.point);
      else if(op==='hazardStart') {frame.danger=frame.control.danger;result.flag=Boolean(frame.danger?.pending);}
      else if(op==='responseUnit') {frame.unit=frame.control.unit;result.flag=Boolean(frame.unit);}
      else if(op==='responseWeapon') result.flag=Boolean(selectedWeapon(frame.control.unit,frame.control));
      else if(op==='hazardAccept') result.flag=Boolean(frame.agent?.acceptsHazard?.());
      else if(op==='responseKind') {const kind=frame.unit.kind;result.number=typeof kind==='number'?kind:NaN;}
      else if(op==='responseDangerRegion') result.flag=Boolean(frame.world.nav?.inDangerRegion?.(frame.unit));
      else if(op==='hazardPoint') frame.responsePoint=frame.danger.point.clone();
      else if(op==='responseResult') {
        frame.response={kind:2,mode:id,point:frame.responsePoint,exit:Boolean(number)};
        if(other!==0)frame.response.alert=true;
      }
      else if(op==='acousticList') result.flag=Boolean(frame.control.acoustic?.length);
      else if(op==='acousticStart') frame.acoustic=frame.control.acoustic[Symbol.iterator]();
      else if(op==='acousticNext') {
        const next=frame.acoustic.next();result.flag=!next.done;
        if(result.flag){const r=next.value,u=frame.responseSource=r?.unit,tag=v=>v===undefined?0:v===null?2:1;
          result.values=u?.alive?[1,u.generation,tag(u.generation),r.generation,tag(r.generation)]:[0,0,0,0,0];}
      } else if(op==='acousticAccept') result.flag=Boolean(frame.agent?.acceptsAcoustic?.(frame.responseSource));
      else if(op==='responseSensor') {
        const u=frame.unit,w=selectedWeapon(u,frame.control);result.flag=Boolean(w);
        if(w){const a=weaponAI(w.def);result.values=[u?.kind,frame.control?.rangeScale??1,a?.maxRange??0,Number(Boolean(a?.implicitRange)),a?.extremeRange??0,
          u?.classDef?.ForwardSpeed??u?.classDef?.MaxSpeed??0,Number(Boolean(a))];}
      } else if(op==='acousticPoint') frame.responsePoint=frame.responseSource.position.clone();
      else if(op==='responseHorizontal') {const p=frame.responsePoint,u=frame.unit.position;result.values=[p.x,u.x,p.z,u.z];}
      else if(op==='responseMountStart') {const u=frame.mountUnit=frame.control.unit;result.flag=Boolean(u);if(u)result.number=u.classDef?.ForwardSpeed??u.classDef?.MaxSpeed??0;}
      else if(op==='responseStay') {const ai=frame.mountAi=frame.control.ai;result.flag=Boolean(ai?.stayInTurrets?.()??frame.world.mission?.stayInTurrets);}
      else if(op==='responseReach') {frame.reach=frame.mountAi?.vehicles?.mountReach??frame.world.vehicles?.mountReach;result.flag=Boolean(frame.reach);}
      else if(op==='responseCanReach') {const reach=frame.reach;result.flag=Boolean(reach(frame.mountUnit,frame.responsePoint));}
      else if(op==='responseShot') {frame.shot=frame.control.shot;result.flag=Boolean(frame.shot?.pending);}
      else if(op==='responseSource') {
        const source=frame.shot.source;let unit=null;
        if(source) {
          if(source.alive!==undefined && source.position)unit=source.alive?source:null;
          else {const u=frame.control.ai?.vehicles?.currentControllable?.(source)??source.remote??source.unit??null;unit=u?.alive?u:null;}
        }
        frame.responseSource=unit;result.flag=Boolean(unit);
      } else if(op==='responsePoint') frame.responsePoint=frame.responseSource.position.clone();
      else if(op==='responseAlertAccept') result.flag=Boolean(frame.agent?.acceptsAlert?.(frame.shot.known,frame.responsePoint));
      else if(op==='responseSight') {
        const multiplier=frame.unit?.kind===5?2:1,target=frame.responseSource;
        result.flag=Boolean(target?.kind===0 && target.soldier);
        if(result.flag){const posture=target.soldier.air?'stand':target.soldier.posture;result.values=[multiplier,({stand:0,crouch:1,prone:2}[posture]??0)];}
        else result.values=[multiplier,target?.sphere?.radius??0];
      } else if(op==='responseDelta') {const p=frame.responsePoint,u=frame.unit.position;result.values=[p.x,u.x,p.y,u.y,p.z,u.z];}
      else if(op==='responseArmed') result.flag=Boolean(frame.unit.combat?.arsenal?.weapons?.length);
      else if(op==='responseDispatch') {const dispatch=frame.dispatch;result.flag=Boolean(dispatch?.(frame.control,frame.unit,frame.responseSource,false,frame.world)?.pattern);}
      else if(op==='responseRandom') frame.random=frame.world.randomB??frame.world.randomA??frame.world.random;
      else if(op==='responseDraw') result.number=frame.random.uniform();
      else if(op==='responsePosition') {const p=frame.unit.position;result.values=[p.x,p.y,p.z];}
      else if(op==='responseDodge') frame.responsePoint=new Vector3(...point);
      else throw Error(`unknown awareness callback ${op}`);
      return result;
    }});
  }
  noise(source,sounds) {
    this.frames.push({source,sounds});
    try {return this.sim.x.ai_noise_radius();} finally {this.frames.pop();}
  }
  broadcast(source,world,sounds) {
    this.frames.push({source,world,sounds});
    try {return this.sim.x.ai_broadcast_acoustic();} finally {this.frames.pop();}
  }
  notifyDanger(world,state,position,owner,enemySkill,onWarn) {
    this.frames.push({world,state,position,owner,enemySkill,onWarn});
    try {return this.sim.x.ai_notify_danger();} finally {this.frames.pop();}
  }
  shot(world,shooter,target,point) {
    this.frames.push({world,shooter,target,point,known:true});
    try {return Boolean(this.sim.x.ai_notify_alert(0));} finally {this.frames.pop();}
  }
  damage(world,target,attacker,packet,known,point) {
    this.frames.push({world,target,attacker,packet,known,point});
    try {return Boolean(this.sim.x.ai_notify_alert(1));} finally {this.frames.pop();}
  }
  hazardResponse(control,world,agent) {
    const frame={control,world,agent,response:null};this.frames.push(frame);
    try {this.sim.x.ai_hazard_response();return frame.response;} finally {this.frames.pop();}
  }
  acousticResponse(control,world,agent) {
    const frame={control,world,agent,response:null};this.frames.push(frame);
    try {this.sim.x.ai_acoustic_response();return frame.response;} finally {this.frames.pop();}
  }
  alertResponse(control,world,agent,dispatch) {
    const frame={control,world,agent,dispatch,response:null};this.frames.push(frame);
    try {this.sim.x.ai_alert_response();return frame.response;} finally {this.frames.pop();}
  }
  rebuild(world) {
    this.frames.push({world});
    try {this.sim.x.ai_rebuild_protected();} finally {this.frames.pop();}
  }
}
