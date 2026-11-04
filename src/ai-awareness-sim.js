// Awareness uses the same AI instance and preserves host query/notification
// order while geometry and recipient iteration run in the module.
import { selectedWeapon } from './ai/classic/weapons-ai.js';

export class NativeAwareness {
  constructor(sim) {
    this.sim=sim;this.frames=[];this.heard=[];this.near=[];this.danger=[];
    sim.bridge.register({aiAwareness:args=>{
      const op=args.op,id=args.id,number=args.number,other=args.other,frame=this.frames[this.frames.length-1];
      const source=frame.source,result={flag:false,number:0,ids:[],values:[]};
      if(op==='alive') result.flag=Boolean(source?.alive);
      else if(op==='motion') {const v=source.velocity;result.values=[Number(Boolean(v)),v?v.x:0,v?v.y:0,v?v.z:0,source.kind];}
      else if(op==='weapon') {
        frame.weapon=source.combat?.arsenal?selectedWeapon(source,source.controller)??source.combat.arsenal.current(0):null;
        result.flag=Boolean(frame.weapon);
      } else if(op==='active') {const w=frame.weapon;result.flag=Boolean(w.charging||w.busy||w.wait>1e-6);}
      else if(op==='mute') result.number=frame.sounds?.(frame.weapon)??frame.weapon.def?.MuteDistance??400;
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
      else if(op==='dangerRow') {const row=frame.world.difficulty?.dangerRow?.();result.flag=row!=null;if(result.flag)result.values=[row[0],row[1]];}
      else if(op==='dangerDraw') result.number=(frame.world.randomA??frame.world.random).range(number,other);
      else if(op==='dangerTime') result.number=frame.world.time??0;
      else if(op==='dangerFirst') result.number=frame.state.firstContact;
      else if(op==='dangerLast') result.number=frame.state.lastNotify;
      else if(op==='dangerQuery') {frame.world.units.query(frame.position,7.5,this.danger,u=>u!==frame.owner);result.ids=this.danger.map(u=>sim.id(u));}
      else if(op==='dangerRecipient') {const u=frame.recipient=sim.objects[id],c=frame.controller=u.controller;result.flag=Boolean(c||u.human);if(result.flag)result.number=u.kind;}
      else if(op==='dangerWarn') frame.onWarn?.(frame.recipient);
      else if(op==='dangerHasNotify') result.flag=Boolean(frame.controller?.notifyDanger);
      else if(op==='dangerNotify') frame.controller.notifyDanger(frame.position);
      else if(op==='dangerWrite') frame.state.lastNotify=number;
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
    this.frames.push({world,state,position,owner,onWarn});
    try {return this.sim.x.ai_notify_danger(enemySkill);} finally {this.frames.pop();}
  }
  rebuild(world) {
    this.frames.push({world});
    try {this.sim.x.ai_rebuild_protected();} finally {this.frames.pop();}
  }
}
