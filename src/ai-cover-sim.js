import { selectedWeapon, weaponAI } from './ai/classic/weapons-ai.js';

export class NativeCover {
  constructor(sim) {
    this.sim=sim;this.frames=[];
    sim.bridge.register({aiCover:args=>{
      const op=args.op,id=args.id,number=args.number,frame=this.frames[this.frames.length-1],a=frame.agent;
      const result={flag:false,number:0,values:[],ids:[]};
      if(op==='scoreStart'){
        const u=frame.unit=a.controller.unit,t=frame.target=a.tracker.point,p=frame.position=u.position,h=frame.hintPosition=frame.hint.position;
        result.values=[t.x,t.y,t.z,p.x,p.y,p.z,h.x,h.y,h.z];
      } else if(op==='forward'){const f=frame.hint.forward;result.values=[f.x,f.y,f.z];}
      else if(op==='rangeWeapon'){const c=frame.rangeController=a.controller;frame.weapon=selectedWeapon(c.unit,c);result.flag=Boolean(frame.weapon);}
      else if(op==='rangeValues'){
        const c=frame.rangeController,u=c.unit,ai=weaponAI(frame.weapon.def);result.flag=Boolean(ai);
        if(ai){const kind=u?.kind;result.values=[typeof kind==='number'?kind:NaN,c?.rangeScale??1,ai.maxRange,Number(Boolean(ai.implicitRange)),ai.extremeRange];}
      } else if(op==='lower'){const ai=weaponAI(selectedWeapon(frame.unit,a.controller)?.def);result.flag=Boolean(ai);if(ai)result.values=[ai.minRange,ai.optimalRange];}
      else if(op==='finish'){const h=frame.hintPosition,p=frame.position;result.values=[h.x,p.x,h.z,p.z];}
      else if(op==='findStart'){frame.controller=a.controller;frame.world=a.world;frame.center=frame.controller.unit.position;}
      else if(op==='query'){const query=frame.query;frame.list=query(a,frame.center,number).slice(0,30);result.ids=frame.list.map(h=>sim.id(h));}
      else if(op==='random')frame.random=frame.world.randomA??frame.world.random;
      else if(op==='time')result.number=frame.world?.time??0;
      else if(op==='draw')result.number=frame.random.int(0,number);
      else if(op==='available'){const available=frame.available;result.flag=Boolean(available(sim.objects[id],frame.controller.unit,number));}
      else if(op==='score'){const score=frame.score;result.number=score(a,sim.objects[id]);}
      else throw Error(`unknown cover callback ${op}`);
      return result;
    }});
  }
  score(agent,hint) {
    this.frames.push({agent,hint});
    try {return this.sim.x.ai_cover_score();} finally {this.frames.pop();}
  }
  find(agent,query,available,score) {
    this.frames.push({agent,query,available,score});
    try {const id=this.sim.x.ai_find_cover();return id?this.sim.objects[id]:null;} finally {this.frames.pop();}
  }
}
