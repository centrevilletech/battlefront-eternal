// Decision ordering and LOD are native. Services read their current live
// objects at each callback, including changes made by earlier services.
import { ClassicScheduler } from './ai/classic/scheduler.js';

export class NativeScheduler extends ClassicScheduler {
  constructor(sim,ai) {
    super(ai);
    this.sim=sim;
    this.inputs=[];
    const field=(object,key,index)=>Object.defineProperty(object,key,{enumerable:true,configurable:true,
      get:()=>sim.x.ai_scheduler_read(index),set:value=>sim.x.ai_scheduler_write(index,value)});
    ['counter','parity','inputParity','skips'].forEach((key,index)=>field(this,key,index));
    ['serviced','skipped','considered','budget'].forEach((key,index)=>field(this.stats,key,index+4));
    sim.bridge.register({aiSchedule:args=>{
      // The bridge reuses its argument record. Copy before any callback
      // can enter another scheduler or navigation export on this instance.
      const op=args.op,object=sim.objects[args.id],number=args.number,phase=args.phase;
      const input=this.inputs[this.inputs.length-1];
      const result={number:0,flag:false,values:[]};
      if(op==='config') { const c=this.config();result.values=[c.cycle,c.losBudget]; }
      else if(op==='notify') ai.vehicles?.notify?.(ai);
      else if(op==='humans') result.values=ai.humanUnits().map(unit=>sim.id(unit));
      else if(op==='broadcast') ai.combat?.broadcastAcoustic?.(object,ai);
      else if(op==='controllers') result.values=ai.controllers.map(c=>sim.id(c));
      else if(op==='controller') result.values=[Number(Boolean(object.processed)),Number(Boolean(object.lodSkipped)),Number(Boolean(object.live)),sim.id(object.unit)];
      else if(op==='processed') object.processed=Boolean(number);
      else if(op==='rebuild') ai.combat?.rebuildProtected?.(ai);
      else if(op==='lodSkip') result.flag=Boolean(this.lodSkip(object));
      else if(op==='service') this.service(object,number);
      else if(op==='plannerIdle') result.flag=Boolean(ai.nav?.plannerIdle?.());
      else if(op==='navService') ai.nav?.service?.(number);
      else if(op==='losService') result.flag=Boolean(ai.combat?.serviceLos?.(ai.world,number)??false);
      else if(op==='phase') object.update(number,phase);
      else if(op==='dropLos') ai.combat?.dropLosRequests?.(object);
      else if(op==='scan') ai.combat?.scan?.(object,ai.world);
      else if(op==='clearCandidate') ai.combat?.clearCandidate?.(object);
      else if(op==='lodDisabled') result.flag=Boolean(ai.lodDisabled());
      else if(op==='identity') result.number=ai.identity(object);
      else if(op==='velocity') {const v=object.velocity;if(v)result.values=[v.x,v.y,v.z];}
      else if(op==='cameras') {
        result.values=[Number(Boolean(ai.world?.networkViews))];
        for(const c of ai.cameras()) {
          const elements=c?.matrixWorld?.elements;
          result.values.push(Number(Boolean(elements)),c?.zoom??1,c?.near??.1,c?.far??Infinity,c?.fov??60,c?.aspect??1,
            ...(elements??new Array(16).fill(0)));
        }
      }
      else if(op==='position') {const p=object.position;result.values=[p.x,p.y,p.z];}
      else if(op==='writeLod') object.lodSkipped=Boolean(number);
      else if(op==='inputAt') result.number=sim.id(ai.controllers[number]);
      else if(op==='executeInput') this.executeInput(object,number);
      else if(op==='budgets') this.serviceBudgets(number);
      else if(op==='visible') result.flag=Boolean(this.visibleOrNear(object));
      else if(op==='inputState') {result.values=[Number(Boolean(object.inputSkip))];result.number=sim.id(input.unit);}
      else if(op==='inputCarry') result.number=object.inputCarry;
      else if(op==='writeInput') {object.inputSkip=Boolean(phase);object.inputCarry=number;}
      else if(op==='markSkip') ai.combat?.markSkip?.(object,Boolean(number));
      else if(op==='intent') input.intent=input.unit.intent;
      else if(op==='consume') ai.combat?.consumeFire?.(object,input.unit,number,input.intent);
      else if(op==='exitPending') result.flag=Boolean(object.exitRequest);
      else if(op==='exit') {input.intent.exit=true;object.exitRequest=false;}
      else if(op==='aim') input.aim=this.aimPoint(object,number);
      else if(op==='navControl') {
        input.control=object.navControl??ai.world.nav?.control?.(object)??null;
        result.flag=Boolean(ai.navigationInput && input.control);
      }
      else if(op==='navInput') ai.navigationInput(input.control,input.unit,input.intent,number,input.aim,{facing:object.facing});
      else if(op==='posture') this.applyPosture(object,input.unit,input.intent);
      else if(op==='jet') {
        input.jet=object.jet;const j=input.jet;
        result.values=j.pending?[1,j.vector.x,j.vector.y,j.vector.z]:[0,0,0,0];
      }
      else if(op==='applyJet') {
        input.intent.jump=true;input.intent.jetDirection=number?input.jet.vector.clone():null;input.jet.pending=false;
      }
      else if(op==='analogs') {
        const out=this.analogOut;out.forward=out.right=out.turn=out.pitch=0;
        if(ai.analogs) {
          ai.analogs(input.unit,object.request,input.aim,number,out,{facing:object.facing});
          input.intent.forward=out.forward;input.intent.right=out.right;input.intent.turn=out.turn;input.intent.pitch=out.pitch;
          result.flag=true;
        }
      }
      else if(op==='fallback') {
        const u=input.unit,s=u.soldier,p=u.position,r=object.request,t=r?.steeringTarget,a=input.aim;
        result.values=[Number(Boolean(s)),p.x,p.y,p.z,s?.aimYaw??0,s?.aimPitch??0,s?.maxSpeed??NaN,s?.maxStrafe??0,s?.maxTurn??NaN,
          u.classDef?.MaxPitchSpeed??.5,Number(Boolean(r?.valid)),t?.x??0,t?.y??0,t?.z??0,Number(Boolean(a)),a?.x??0,a?.y??0,a?.z??0,Number(Boolean(object.facing))];
      }
      else if(op==='applyFallback') {
        const a=sim.out();const out=this.analogOut;
        out.forward=a[0];out.right=a[1];out.turn=a[2];out.pitch=a[3];
        input.intent.forward=out.forward;input.intent.right=out.right;input.intent.turn=out.turn;input.intent.pitch=out.pitch;
      }
      else if(op==='kind') result.number=input.unit.kind;
      else if(op==='droideka') this.applyDroidekaMode(object,input.unit,input.intent);
      else if(op==='dodge') result.number=object.dodge;
      else if(op==='applyDodge') {input.intent.jump=true;input.intent.right=number;input.intent.forward=0;object.dodge=0;}
      else if(op==='postureInfo') {
        const s=input.unit.soldier,have=s?.posture;
        result.values=[Number(Boolean(s)),input.unit.kind,object.navControl?.stance??object.stance,
          !have?0:have==='stand'?1:have==='crouch'?2:have==='prone'?3:4];
      }
      else if(op==='crouch') input.intent.crouch=true;
      else if(op==='prone') input.intent.prone=true;
      else if(op==='droidekaInfo') {
        const s=input.unit.soldier;
        result.values=[Number(Boolean(s)),s?.mode,Number(s?.rolled===false),object.wantedMode];
      }
      else if(op==='use') input.intent.use=true;
      else throw Error(`unknown AI scheduler callback ${op}`);
      return result;
    }});
  }
  update(dt) { this.sim.x.ai_scheduler_command(0,0,dt); }
  service(controller,dt) { this.sim.x.ai_scheduler_command(1,this.sim.id(controller),dt); }
  serviceBudgets(budget) { this.sim.x.ai_scheduler_command(2,0,budget); }
  lodSkip(controller) { return Boolean(this.sim.x.ai_scheduler_command(3,this.sim.id(controller),0)); }
  visibleOrNear(unit) { return Boolean(this.sim.x.ai_scheduler_command(4,this.sim.id(unit),0)); }
  executeInputs(dt) { this.sim.x.ai_scheduler_command(5,0,dt); }
  withInput(controller,unit,intent,op,dt=0) {
    this.inputs.push({unit,intent});
    try {return this.sim.x.ai_scheduler_command(op,this.sim.id(controller),dt);}
    finally {this.inputs.pop();}
  }
  executeInput(controller,dt) {return Boolean(this.withInput(controller,controller.unit,null,6,dt));}
  applyPosture(controller,unit,intent) {this.withInput(controller,unit,intent,7);}
  applyDroidekaMode(controller,unit,intent) {this.withInput(controller,unit,intent,8);}
}
