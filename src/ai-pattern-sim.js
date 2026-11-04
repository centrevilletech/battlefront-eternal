const fields=['probability','coverRetry','prior','pathActive','crouch','paused','pauseUntil','toward','side',
  'bandCheckAt','stepAt','facing','stepped','lateralAt','lateralFacing','lastPath','retreatAt','coverHint',
  'jetting','jetRetry','jetStart','jetHint','hintJetRetry','searched','skipFire'];
const actions={2:'standing',3:'pauseAndMove',4:'bandFlank',5:'lateral',6:'melee',8:'retreat',9:'inCover',
  10:'jetTick',11:'hintJetTick',12:'lostTarget',20:'reposition',21:'flank',22:'directJet'};

export class NativePatterns {
  constructor(sim) {
    this.sim=sim;this.frames=[];this.patterns=new WeakMap();
    sim.bridge.register({aiPattern:args=>{
      const op=args.op,id=args.id,number=args.number,other=args.other,frame=this.frames[this.frames.length-1],h=frame.pattern,a=frame.agent;
      const result={flag:false,number:0,other:0};
      if(op==='field')result.number=h[fields[id]];
      else if(op==='initValue')result.number=frame.probability;
      else if(op==='request')h.requestState(id===1?frame.value:id===2?frame.prior:number);
      else if(op==='flush')h.flush();
      else if(op==='priorCapture')frame.prior=h.prior;
      else if(op==='random')frame.random=h.random;
      else if(op==='time')result.number=h.world?.time??0;
      else if(op==='int'){const random=id===1?h.random:frame.random;frame.draw=random.int(number,other);result.flag=typeof frame.draw==='number';if(result.flag)result.number=frame.draw;}
      else if(op==='add'){if(id===1)result.flag=Boolean(frame.draw);else result.number=number+frame.draw;}
      else if(op==='agent')frame.agent=h.agent;
      else if(op==='state'){const value=h.state;result.number=typeof value==='number'?value:NaN;}
      else if(op==='stateValue')result.number=h.state;
      else if(op==='stop')a.helper?.stop?.();
      else if(op==='roll')result.number=h.controller.roll??0;
      else if(op==='action')frame.result=id===9||id===11?h[actions[id]](frame.value):h[actions[id]]();
      else if(op==='coverInit')h.cover.init(h.coverHint);
      else if(op==='jetEnter')h.world.nav?.jetHints?.enter?.(h.controller,h.jetHint);
      else if(op==='delay'){if(id===0)a.tracker.defaultDelay(number);else a.tracker.setDelay(id,number);}
      else if(op==='gotoTarget'){const helper=a.helper,point=a.tracker.point;helper?.goto?.(point,undefined);}
      else if(op==='vehicleEnter')h.world.vehicles?.enterPattern?.(h,h.state);
      else if(op==='stance'){const c=h.controller;if(c.setStance)c.setStance(0);else c.stance=0;}
      else if(op==='coverExit'){h.cover.onExit();h.cover.cleanup();h.cover.state=0;h.cover.pending=0;}
      else if(op==='jetExit')h.world.nav?.jetHints?.exit?.(h.controller,h.jetHint);
      else if(op==='vehicleExit')h.world.vehicles?.exitPattern?.(h,h.state);
      else if(op==='vehicleTick')h.world.vehicles?.tickPattern?.(h,h.state,frame.value);
      else if(op==='cleanup')h.cover.cleanup();
      else if(op==='range'){frame.chance=frame.random.range(0,1);frame.coverProbability=h.probability;
        result.flag=typeof frame.chance==='number' && typeof frame.coverProbability==='number';if(result.flag){result.number=frame.chance;result.other=frame.coverProbability;}}
      else if(op==='rangeCompare')result.flag=frame.chance<frame.coverProbability;
      else if(op==='findCover'){const find=frame.find;frame.hint=find(h.agent);h.coverHint=frame.hint;result.flag=Boolean(frame.hint);}
      else if(op==='reserve'){const reserve=frame.reserve;reserve(frame.hint,h.controller.unit,number);}
      else if(op==='priorFrom')h.prior=frame.value;
      else if(op==='jetUnit'){frame.unit=h.controller.unit;result.flag=frame.unit?.kind===0;}
      else if(op==='jetJump'){const unit=frame.unit;frame.jet=unit.soldier?.jet;frame.cls=unit.classDef||{};result.number=frame.jet?.jump??frame.cls.JetJump??0;}
      else if(op==='jetBorder')result.number=frame.jet?.minBorder??frame.cls.JetFuelMinBorder??0;
      else if(op==='jetFuel')result.number=frame.unit.soldier?.fuel??0;
      else if(op==='jetReady')result.flag=Boolean(h.jetReady());
      else if(op==='jetHints'){frame.jetHints=h.world.nav?.jetHints;result.flag=Boolean(frame.jetHints?.excluded?.(h.controller));}
      else if(op==='hintFind'){const hint=frame.jetHints?.find?.(h.controller)??null;h.jetHint=hint;result.flag=Boolean(hint);}
      else if(op==='sampleTime')result.number=h.agent.tracker.sampleTime;
      else throw Error(`unknown pattern callback ${op}`);
      return result;
    }});
  }
  bind(pattern) {
    if(this.patterns.has(pattern))return;
    const x=this.sim.x,id=x.ai_pattern_new(),opaque=[];
    fields.forEach((name,index)=>{
      const initial=pattern[name];
      Object.defineProperty(pattern,name,{enumerable:true,configurable:true,
        get:()=>{const kind=x.ai_pattern_kind(id,index);return kind===0?x.ai_pattern_field(id,index):kind===2?false:kind===3?true:opaque[index];},
        set:value=>{let kind=typeof value==='number'?0:value===false?2:value===true?3:1;
          opaque[index]=kind===1?value:undefined;
          const reference=kind===1 && value && (typeof value==='object'||typeof value==='function');
          x.ai_pattern_write(id,index,kind,kind===0?value:reference?this.sim.id(value):0);}});
      pattern[name]=initial;
    });
    this.patterns.set(pattern,id);
  }
  run(pattern,mode,value,probability,find,reserve) {
    this.bind(pattern);const frame={pattern,value,probability,find,reserve};this.frames.push(frame);
    try {const flag=this.sim.x.ai_pattern_run(this.patterns.get(pattern),mode);return mode===4?frame.result:mode>=6?Boolean(flag):undefined;} finally {this.frames.pop();}
  }
}
