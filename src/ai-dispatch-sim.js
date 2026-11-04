export class NativeDispatch {
  constructor(sim) {
    this.sim=sim;this.frames=[];
    sim.bridge.register({aiDispatch:args=>{
      const op=args.op,number=args.number,frame=this.frames[this.frames.length-1],{controller:c,unit,target,world,out}=frame;
      const result={flag:false,number:0,values:[],text:''},strict=value=>typeof value==='number'?value:NaN;
      if(op==='init'){out.pattern=0;out.probability=0;out.aux=false;out.action=6;out.inBand=false;out.weapon=null;result.flag=Boolean(unit&&target);}
      else if(op==='kinds')result.values=[strict(unit.kind),strict(target.kind)];
      else if(op==='roleStart')frame.classDef=unit.classDef;
      else if(op==='roleAI')result.number=strict(frame.classDef?.ai?.role);
      else if(op==='rolePlain')result.number=strict(frame.classDef?.role);
      else if(op==='roleName')result.text=String(frame.classDef?.UnitType??'').trim().toLowerCase();
      else if(op==='ownerHero')result.flag=Boolean(unit.hero);
      else if(op==='targetHero')result.flag=Boolean(target.hero);
      else if(op==='select'){
        const select=frame.select;select(c,unit,target,frame.switchAllowed,world,out);
      } else if(op==='aux')result.flag=Boolean(out.aux);
      else if(op==='action')result.number=strict(out.action);
      else if(op==='numericAction')result.number=out.action;
      else if(op==='inBand')result.flag=Boolean(out.inBand);
      else if(op==='pattern')out.pattern=number;
      else if(op==='probability')out.probability=number;
      else if(op==='roll')result.number=c?.roll??0;
      else if(op==='vehicleHook'){frame.hook=world?.vehicles?.dispatch;result.flag=Boolean(frame.hook);}
      else if(op==='vehicleDispatch')frame.result=frame.hook.call(world.vehicles,c,unit,target,frame.switchAllowed,world,out);
      else if(op==='rangeStart'){result.flag=Boolean(out.inBand);out.pattern=0;result.number=strict(out.action);}
      else if(op==='difficulty'){const difficulty=world?.difficulty;frame.skill=difficulty?.enemySkill??14;result.flag=Boolean(difficulty?.hard??difficulty?.tier===3);}
      else if(op==='skill')result.number=frame.skill;
      else throw Error(`unknown engagement callback ${op}`);
      return result;
    }});
  }
  run(controller,unit,target,switchAllowed,world,out,select) {
    const frame={controller,unit,target,switchAllowed,world,out,select,result:out};this.frames.push(frame);
    try {this.sim.x.ai_dispatch();return frame.result;} finally {this.frames.pop();}
  }
  rangePolicy(controller,world,out) {
    this.frames.push({controller,world,out});
    try {this.sim.x.ai_range_policy();return out;} finally {this.frames.pop();}
  }
}
