import { selectedWeapon } from './ai/classic/weapons-ai.js';

// The generator keeps IteratorClose behavior when a weapon setter throws.
function* weapons(arsenal) {for(const weapon of arsenal?.weapons||[])yield weapon;}

export class NativeBindings {
  constructor(sim) {
    this.sim=sim;this.frames=[];this.cells=new WeakSet();
    sim.bridge.register({aiBinding:args=>{
      const op=args.op,id=args.id,frame=this.frames[this.frames.length-1],{binding,controller,world,ai,services}=frame;
      const result={flag:false,number:0,values:[]};
      if(op==='create')frame.result=services.create(controller);
      else if(op==='reset')frame.result.reset();
      else if(op==='arsenal'){frame.arsenal=controller.unit?.combat?.arsenal;result.flag=Boolean(frame.arsenal);}
      else if(op==='weapons'){const random=services.rng(world);frame.draw=()=>random.uniform();frame.iterator=weapons(frame.arsenal);}
      else if(op==='nextWeapon'){const next=frame.iterator.next();result.flag=!next.done;frame.weapon=next.value;}
      else if(op==='weaponRandom')frame.weapon.random=frame.draw;
      else if(op==='hasPerception')result.flag=Boolean(controller.perception);
      else if(op==='clearCandidate')controller.perception.candidate=null;
      else if(op==='unit'){frame.unit=controller.unit;result.flag=Boolean(frame.unit);}
      else if(op==='weapon'){frame.weapon=selectedWeapon(frame.unit,controller);result.flag=Boolean(frame.weapon);}
      else if(op==='lead'){const lead=services.lead;lead(frame.dt,frame.unit,frame.target,frame.weapon,frame.out);}
      else if(op==='result')frame.result=id===2?null:Boolean(id);
      else if(op==='fire'){const fire=services.fire;fire(controller,frame.unit,world);}
      else if(op==='hooked')result.flag=Boolean(binding.aimHooked);
      else if(op==='hasIntent')result.flag=Boolean(frame.intent);
      else if(op==='update')frame.result=binding.updateAim(controller,frame.dt);
      else if(op==='intentWrite')frame.intent.aim=frame.result;
      else if(op==='hook')binding.aimHooked=true;
      else if(op==='intent'){frame.intent=controller.unit?.intent;result.flag=Boolean(frame.intent);}
      else if(op==='aimStart'){frame.unit=controller.unit;frame.perception=controller.perception;result.flag=Boolean(frame.unit?.alive&&frame.perception);}
      else if(op==='forward')frame.unit.forward(services.aimDesired);
      else if(op==='difficulty'){frame.difficulty=controller.difficulty;result.flag=Boolean(frame.difficulty);}
      else if(op==='curveA'){const a=frame.difficulty.A;result.number=typeof a==='number'?a:NaN;}
      else if(op==='ownCurves')frame.curves=frame.difficulty;
      else if(op==='aiCurves'){frame.method=ai?.curvesFor;result.flag=frame.method!=null;}
      else if(op==='aiCurvesCall'){frame.curves=Reflect.apply(frame.method,ai,[frame.unit.team]);result.flag=frame.curves!=null;}
      else if(op==='difficultyCurves'){frame.method=frame.difficulty?.curves;result.flag=frame.method!=null;}
      else if(op==='difficultyCurvesCall'){frame.curves=Reflect.apply(frame.method,frame.difficulty,[frame.unit.team]);result.flag=frame.curves!=null;}
      else if(op==='nullCurves')frame.curves=null;
      else if(op==='aimer'){frame.aimer=frame.perception.aimer;frame.method=frame.aimer.update;}
      else if(op==='aimerUpdate')frame.result=Reflect.apply(frame.method,frame.aimer,[services.aimDesired,frame.dt,frame.curves,frame.weapon,frame.unit.hero]);
      else if(op==='reader'){frame.reader=binding.reader;result.flag=frame.reader!=null;}
      else if(op==='read')frame.reader.read(event=>this.event(world,event,services));
      else if(op==='dangerState'){frame.state=frame.round.aiDanger;result.flag=Boolean(frame.state);}
      else if(op==='dangerNew'){const state=frame.state={firstContact:Infinity,lastNotify:-1};frame.round.aiDanger=state;}
      else if(op==='dangerBind')this.danger(frame.state);
      else if(op==='dangerContact'){frame.previous=frame.state.firstContact;result.flag=typeof frame.firstContact==='number'&&typeof frame.previous==='number';if(result.flag)result.values=[frame.firstContact,frame.previous];}
      else if(op==='dangerCompare')result.flag=frame.firstContact<frame.previous;
      else if(op==='dangerWrite')frame.state.firstContact=frame.firstContact;
      else if(op==='dangerNotify'){
        const skill=world.difficulty?.enemySkill??14,notify=services.danger,emit=services.emit;
        frame.result=notify(world,frame.state,frame.position,frame.owner,skill,u=>emit(ai,u,{event:'flee',property:'FleeSound'}));
      }
      else if(op==='eventKind')result.flag=frame.event.kind===(id===0?'shot':'damage');
      else if(op==='eventSource')frame.refs[id]=frame.event.source;
      else if(op==='eventTarget')frame.refs[id]=frame.event.target;
      else if(op==='eventAttacker')frame.refs[id]=frame.event.attacker;
      else if(op==='reference')result.flag=Boolean(frame.refs[id]);
      else if(op==='referenceUnit')result.flag=Boolean(frame.refs[id].unit);
      else if(op==='resolveUnit'){frame.resolved=frame.refs[id]?.unit;result.flag=Boolean(frame.resolved);}
      else if(op==='resolveAlive')result.flag=Boolean(frame.resolved.alive);
      else if(op==='resolveGeneration')result.flag=frame.resolved.generation===frame.refs[id].generation;
      else if(op==='resolveResult')frame.refs[id]=frame.resolved;
      else if(op==='referenceHasAlive')result.flag=frame.refs[id].alive!==undefined;
      else if(op==='referenceAlive')result.flag=Boolean(frame.refs[id].alive);
      else if(op==='referenceNull')frame.refs[id]=null;
      else if(op==='shotController')frame.shotController=frame.refs[0]?.controller;
      else if(op==='shotFallback')result.flag=Boolean(frame.shotController?.perception?.fire);
      else if(op==='shotReference')frame.refs[id]=frame.shotController.target;
      else if(op==='pair')result.flag=Boolean(frame.refs[0]&&frame.refs[1]);
      else if(op==='shot'){const notify=services.shot;notify(world,frame.refs[0],frame.refs[1],frame.refs[0].position);}
      else if(op==='attackerFallback')frame.refs[id]=frame.event.attackerCharacter??null;
      else if(op==='damage'){
        const e=frame.event,notify=services.damage,vec=services.vec;
        notify(world,frame.refs[0],frame.refs[1],{amount:e.amount,generic:e.generic??1,typed:e.typed??1},Boolean(e.known),e.point?vec(e.point):null);
      }
      else throw Error(`unknown AI binding callback ${op}`);
      return result;
    }});
  }
  fields(object,names) {
    if(!object||(typeof object!=='object'&&typeof object!=='function')||this.cells.has(object))return;
    const descriptors=names.map(name=>Object.getOwnPropertyDescriptor(object,name));
    if(descriptors.some(d=>!d?.configurable||!d.writable||d.get||d.set))return;
    const x=this.sim.x,id=x.ai_binding_new(),opaque=[];
    names.forEach((name,index)=>{
      Object.defineProperty(object,name,{enumerable:descriptors[index].enumerable,configurable:true,
        get:()=>{const kind=x.ai_binding_kind(id,index);return kind===0?x.ai_binding_field(id,index):kind===2?false:kind===3?true:opaque[index];},
        set:value=>{const kind=typeof value==='number'?0:value===false?2:value===true?3:1;opaque[index]=kind===1?value:undefined;
          const reference=kind===1&&value&&(typeof value==='object'||typeof value==='function');x.ai_binding_write(id,index,kind,kind===0?value:reference?this.sim.id(value):0);}});
      object[name]=descriptors[index].value;
    });this.cells.add(object);
  }
  bind(binding) {if(this.sim.navigation)this.fields(binding,['aimHooked','reader']);}
  danger(state) {this.fields(state,['firstContact','lastNotify']);}
  run(mode,binding,world,ai,services,args) {
    this.bind(binding);const frame={binding,world,ai,services,...args};this.frames.push(frame);
    try{this.sim.x.ai_combat_binding(mode);return frame.result;}
    catch(error){try{frame.iterator?.return?.();}catch{}throw error;}
    finally{this.frames.pop();}
  }
  event(world,event,services) {
    this.frames.push({world,event,services,refs:[]});
    try{this.sim.x.ai_combat_binding(9);}finally{this.frames.pop();}
  }
}
