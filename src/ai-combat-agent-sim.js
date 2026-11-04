import { Vector3 } from 'three';
import { forwardSpeed, sensorRange, switchWeapon } from './ai/classic/weapons-ai.js';
const fields=['alignAt','looks','lookAngle','aligned','hazardUntil','proneUntil','pattern','stationary','point','hazard'];

export class NativeCombatAgents {
  constructor(sim) {
    this.sim=sim;this.frames=[];this.agents=new WeakMap();this.vectors=new WeakSet();
    sim.bridge.register({aiCombatAgent:args=>{
      const op=args.op,id=args.id,number=args.number,point=[...args.point],frame=this.frames[this.frames.length-1],a=frame.agent;
      const result={flag:false,number:0,values:[]},strict=value=>typeof value==='number'?value:NaN;
      if(op==='goalMode')result.number=strict(frame.value?.mode);
      else if(op==='request')a.requestState(number);
      else if(op==='goalPoint'){const goal=frame.value;if(goal.point)(id===0?a.point:a.hazard).copy(goal.point);}
      else if(op==='goalTarget'){const resolve=frame.resolve;a.tracker.assign(resolve(frame.value.refA));}
      else if(op==='cleanup')a.patterns.cleanup();
      else if(op==='base')frame.result=frame.base();
      else if(op==='mode')a.controller.wantedMode=number;
      else if(op==='state')result.number=strict(a.state);
      else if(op==='patternExit')a.patterns.onExit();
      else if(op==='unit'){frame.unit=a.controller.unit;result.flag=Boolean(frame.unit);result.number=strict(frame.unit?.kind);}
      else if(op==='speed')result.number=forwardSpeed(frame.unit);
      else if(op==='mount')result.flag=Boolean(a.mountUnreachable(frame.point));
      else if(op==='known')result.flag=Boolean(frame.value);
      else if(op==='cone'){const cone=frame.cone;result.flag=Boolean(cone(frame.unit,frame.point));}
      else if(op==='sourcePoint'){result.flag=Boolean(frame.value);if(result.flag)frame.point=frame.value.position;}
      else if(op==='target')result.flag=Boolean(a.tracker.target);
      else if(op==='distanceStart'){const p=frame.position=frame.unit.position,t=frame.targetPoint=a.tracker.point,q=frame.point;result.values=[q.x,p.x,q.z,p.z];}
      else if(op==='distanceTarget'){const p=frame.position,t=frame.targetPoint;result.values=[t.x,p.x,t.z,p.z];}
      else if(op==='engage')frame.result=a.canEngage(frame.value);
      else if(op==='mountReach'){frame.reach=a.ai?.vehicles?.mountReach;result.flag=Boolean(frame.reach);}
      else if(op==='mountCall'){const reach=frame.reach;result.flag=Boolean(reach(a.controller.unit,frame.value));}
      else if(op==='boarding'){const clear=frame.clear;result.flag=Boolean(clear(a.world,a.controller.unit,frame.value));}
      else if(op==='shield'){frame.shield=frame.value.soldier?.shield;result.flag=Boolean(frame.shield&&!frame.shield.on);}
      else if(op==='shieldMax')result.number=frame.shield.max??0;
      else if(op==='shieldValue')result.number=frame.shield.value??0;
      else if(op==='switchShield')switchWeapon(a.controller,frame.value,1);
      else if(op==='fireA')a.controller.fireA=true;
      else if(op==='fireB')a.controller.fireB=false;
      else if(op==='entryStart'){frame.controller=a.controller;frame.unit=frame.controller.unit;result.number=a.world?.time??0;}
      else if(op==='capturedKind')result.number=strict(frame.unit?.kind);
      else if(op==='capturedMode')frame.controller.wantedMode=number;
      else if(op==='stop')a.helper?.stop?.();
      else if(op==='investigateStart'){const p=frame.position=frame.unit.position;result.values=[a.point.x,p.x,a.point.z,p.z];}
      else if(op==='investigateNormal'){const n=frame.scratch.cn;n.x=point[0];n.y=point[1];n.z=point[2];}
      else if(op==='investigateValues'){const p=frame.position,n=frame.scratch.cn;result.values=[p.x,n.x,p.y,n.y,p.z,n.z];}
      else if(op==='investigatePoint')a.point.set(...point);
      else if(op==='look'){const p=a.point,c=frame.controller;if(c.lookAt)c.lookAt(p);else{c.aimState=1;(c.aimPoint||=new Vector3()).copy(p);}}
      else if(op==='stance'){const c=frame.controller;if(c.setStance)c.setStance(number);else c.stance=number;}
      else if(op==='nextLook')a.nextLook();
      else if(op==='captureTarget'){frame.target=a.tracker.target;result.flag=Boolean(frame.target);}
      else if(op==='dispatch'){const dispatch=frame.dispatch;frame.selection=dispatch(frame.controller,frame.unit,frame.target,true,id===0?a.world:frame.world,frame.scratch.pick);}
      else if(op==='storePattern')a.pattern=frame.selection.pattern;
      else if(op==='hasPattern')result.flag=Boolean(frame.selection.pattern);
      else if(op==='initPattern')a.patterns.init(frame.selection.pattern,frame.selection.probability);
      else if(op==='evade')a.evade(number);
      else if(op==='alongPath'){const c=frame.controller;if(c.lookAlongPath)c.lookAlongPath();else c.aimState=2;}
      else if(op==='behaveStart'){frame.controller=a.controller;frame.unit=frame.controller.unit;frame.world=a.world;result.flag=Boolean(frame.unit?.alive);}
      else if(op==='capturedTime')result.number=frame.world?.time??0;
      else if(op==='select')result.flag=Boolean(a.tracker.select(frame.world));
      else if(op==='assignWeapon')a.tracker.assignWeaponTarget(frame.world);
      else if(op==='alert')frame.controller.alertAnim=true;
      else if(op==='jetState')result.flag=a.patterns.state===11;
      else if(op==='flying')result.flag=Boolean(a.helper?.flying);
      else if(op==='interrupt')result.flag=Boolean(a.interrupts?.tick?.());
      else if(op==='aligned')result.flag=Boolean(a.alignedTo(a.point));
      else if(op==='field')result.number=a[fields[id]];
      else if(op==='fieldTruthy')result.flag=Boolean(a[fields[id]]);
      else if(op==='fastTurn'){const c=frame.controller;if(c.requestFacing)c.requestFacing();else if(c.unit?.kind===0||c.unit?.kind===3)c.facing=true;}
      else if(op==='patternUpdate')a.patterns.update(frame.value,false);
      else if(op==='sensorStart'){frame.position=frame.unit.position;frame.targetPoint=a.tracker.point;result.number=sensorRange(frame.unit,frame.controller);}
      else if(op==='sensorDistance'){const p=frame.position,t=frame.targetPoint;result.values=[t.x,p.x,t.z,p.z];}
      else if(op==='samePattern')result.flag=frame.selection.pattern===a.pattern;
      else if(op==='skipFire')result.flag=Boolean(a.patterns.skipFire);
      else if(op==='raiseShield')result.flag=Boolean(a.raiseShield(frame.unit));
      else if(op==='helperTick')a.helper?.tick?.(frame.value);
      else if(op==='fireControls')a.tracker.fireControls(frame.world);
      else throw Error(`unknown combat-agent callback ${op}`);
      return result;
    }});
  }
  bindVector(vector) {
    if(!(vector instanceof Vector3) || this.vectors.has(vector))return;
    const names=['x','y','z'];
    if(names.some(name=>{const d=Object.getOwnPropertyDescriptor(vector,name);return !d?.configurable || d.get || d.set;}))return;
    const x=this.sim.x,id=x.ai_combat_agent_vector_new(),opaque=[],initial=names.map(name=>vector[name]);
    names.forEach((name,index)=>{
      Object.defineProperty(vector,name,{enumerable:true,configurable:true,
        get:()=>{const kind=x.ai_combat_agent_vector_kind(id,index);return kind===0?x.ai_combat_agent_vector(id,index):kind===2?false:kind===3?true:opaque[index];},
        set:value=>{const kind=typeof value==='number'?0:value===false?2:value===true?3:1;opaque[index]=kind===1?value:undefined;x.ai_combat_agent_vector_write(id,index,kind,kind===0?value:0);}});
      vector[name]=initial[index];
    });this.vectors.add(vector);
  }
  bind(agent) {
    if(this.agents.has(agent))return;
    const x=this.sim.x,id=x.ai_combat_agent_new(),opaque=[];
    fields.forEach((name,index)=>{
      const initial=agent[name];
      Object.defineProperty(agent,name,{enumerable:true,configurable:true,
        get:()=>{const kind=x.ai_combat_agent_kind(id,index);return kind===0?x.ai_combat_agent_field(id,index):kind===2?false:kind===3?true:opaque[index];},
        set:value=>{const kind=typeof value==='number'?0:value===false?2:value===true?3:1;opaque[index]=kind===1?value:undefined;
          if(index>=8)this.bindVector(value);const reference=kind===1&&value&&(typeof value==='object'||typeof value==='function');
          x.ai_combat_agent_write(id,index,kind,kind===0?value:reference?this.sim.id(value):0);}});
      agent[name]=initial;
    });this.agents.set(agent,id);
  }
  run(agent,mode,value,point,base,resolve,cone,clear,scratch,dispatch) {
    this.bind(agent);const frame={agent,value,point,base,resolve,cone,clear,scratch,dispatch};this.frames.push(frame);
    try {const result=this.sim.x.ai_combat_agent_run(this.agents.get(agent),mode);return mode<3||mode>8?undefined:result===2?frame.result:Boolean(result);} finally {this.frames.pop();}
  }
}
