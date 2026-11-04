import { selectedWeapon, weaponAI } from './ai/classic/weapons-ai.js';
import { Vector3 } from 'three';

export class NativeCover {
  constructor(sim) {
    this.sim=sim;this.frames=[];this.helpers=new WeakMap();
    sim.bridge.register({aiCover:args=>{
      const op=args.op,id=args.id,number=args.number,other=args.other,point=[...args.point],frame=this.frames[this.frames.length-1],a=frame.agent,h=frame.helper;
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
      else if(op==='helperTime')result.number=h.world?.time??0;
      else if(op==='helperCapture'){frame.agent=h.agent;result.number=h.world?.time??0;}
      else if(op==='helperState'){const state=h.state;result.number=typeof state==='number'?state:NaN;}
      else if(op==='helperStateValue')result.number=h.state;
      else if(op==='helperHint')h.hint=id===1?frame.hintIn:id===2?frame.next:null;
      else if(op==='helperReserveInit'){const reserve=frame.reserve;reserve(frame.hintIn,h.agent.controller.unit,number);}
      else if(op==='helperRenewPrepare'){frame.heldHint=h.hint;frame.heldOwner=h.agent.controller.unit;result.number=h.world?.time??0;}
      else if(op==='helperReserveHeld'){const reserve=frame.reserve;reserve(frame.heldHint,frame.heldOwner,number);}
      else if(op==='helperRenew')h.renew();
      else if(op==='helperRequest')h.requestState(number);
      else if(op==='helperFlush')h.flush();
      else if(op==='helperGoto'){const move=a.helper,p=h.hint.position;move?.goto?.(p,undefined);}
      else if(op==='helperFreeze')a.tracker.freeze(Boolean(id),number);
      else if(op==='helperDelay'){if(id===0)a.tracker.defaultDelay(number);else a.tracker.setDelay(id,number);}
      else if(op==='helperInt'){frame.draw=h.random.int(number,other);result.flag=typeof frame.draw==='number';if(result.flag)result.number=frame.draw;}
      else if(op==='helperAdd')result.number=number+frame.draw;
      else if(op==='helperMaskStart'){frame.drawRandom=h.random;frame.mask=id===0?h.hint.primaryMask??1:id===1?h.hint.secondaryMask??1:h.hint.sideMask??0;}
      else if(op==='helperMask')result.number=frame.mask;
      else if(op==='helperMaskDraw'){frame.draw=frame.drawRandom.int(0,number);result.number=frame.draw;result.flag=Boolean(frame.draw);}
      else if(op==='helperPostureDraw')h.posture=frame.draw;
      else if(op==='helperField')result.number=h[['hint','posture','deadline','checkAt','reselectAt'][id]];
      else if(op==='helperScore'){const score=frame.score;result.number=score(a,h.hint);}
      else if(op==='helperRelease'){const release=frame.release;release(h.hint);}
      else if(op==='helperFind'){const find=frame.find;frame.next=find(a);result.flag=Boolean(frame.next);}
      else if(op==='helperArrived')result.flag=Boolean(a.helper?.arrived);
      else if(op==='helperSideMask')result.number=h.hint.sideMask??0;
      else if(op==='helperSideForward'){const f=h.hint.forward;result.values=[f.x,f.y,f.z];}
      else if(op==='helperSidePosition'){frame.move=a.helper;const p=h.hint.position;result.values=[p.x,p.y,p.z];}
      else if(op==='helperGotoPoint')frame.move?.goto?.(new Vector3(...point),undefined);
      else throw Error(`unknown cover callback ${op}`);
      return result;
    }});
  }
  bind(helper) {
    if(this.helpers.has(helper))return;
    const x=this.sim.x,id=x.ai_cover_new(),opaque=[];
    ['hint','posture','deadline','checkAt','reselectAt'].forEach((name,index)=>{
      const initial=helper[name];
      Object.defineProperty(helper,name,{enumerable:true,configurable:true,
        get:()=>x.ai_cover_kind(id,index)===0?x.ai_cover_field(id,index):opaque[index],
        set:value=>{if(typeof value==='number'){opaque[index]=undefined;x.ai_cover_write(id,index,0,value);}else{opaque[index]=value;
          const reference=index===0 && value && (typeof value==='object'||typeof value==='function');x.ai_cover_write(id,index,1,reference?this.sim.id(value):0);}}});
      helper[name]=initial;
    });
    this.helpers.set(helper,id);
  }
  reserve(hint,owner,at,reservations,ring) {
    const sim=this.sim,x=sim.x,ownerId=sim.id(owner);
    for(const h of ring(hint)) {
      if(h===null || (typeof h!=='object' && typeof h!=='function'))reservations.set(h,null);
      const id=sim.id(h);x.ai_cover_reserve(id,ownerId,at);
      reservations.set(h,{
        get owner(){const ownerId=x.ai_cover_reservation_owner(id);return ownerId?sim.objects[ownerId]:owner;},
        get at(){return x.ai_cover_reservation_at(id);},
        set at(value){x.ai_cover_reservation_write(id,value);},
        nativeAvailable(owner,at){return Boolean(x.ai_cover_available(id,sim.id(owner),at));},
      });
    }
  }
  helper(helper,mode,hintIn,reserve,release,score,find) {
    this.bind(helper);this.frames.push({helper,hintIn,reserve,release,score,find});
    try {this.sim.x.ai_cover_helper(this.helpers.get(helper),mode);} finally {this.frames.pop();}
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
